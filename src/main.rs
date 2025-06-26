mod processor;

use std::{
    path::PathBuf,
    sync::Arc,
    time::Instant,
    env,
};

use actix_multipart::Multipart;
use actix_web::{
    App, HttpResponse, HttpServer, Result as ActixResult,
    error::ResponseError,
    get, post, web,
    http::{StatusCode, header::ContentType},
    middleware::{Logger, DefaultHeaders},
    dev::{ServiceRequest, ServiceResponse, forward_ready, Transform},
    Error as ActixError,
};
use extractous::Extractor;
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::File as AsyncFile;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn, error, debug, instrument};
use actix_cors::Cors;
use futures_util::future::{ok, Ready, LocalBoxFuture};

use processor::create_extractor;
use uuid::Uuid;

/// Sanitizes a filename by replacing non-ASCII characters with safe alternatives
fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else if c.is_whitespace() {
                '_'
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Represents a list of URLs to be parsed.
#[derive(Debug, Deserialize)]
struct UrlListRequest {
    urls: Vec<String>,
}

/// Represents the extracted content for a single URL or file.
#[derive(Debug, Serialize, Deserialize)]
struct ExtractionResult {
    id: String,
    /// For URLs: the original URL
    url: Option<String>,
    /// For files: the original filename as uploaded by the client
    file_name: Option<String>,
    /// The index of this file/URL in the original request (0-based)
    /// This allows clients to map results back to their input
    input_index: usize,
    extracted_text: String,
    metadata: serde_json::Value,
    processing_time_ms: u64,
    error: Option<String>,
}

/// Represents the overall response for multiple extractions.
#[derive(Debug, Serialize, Deserialize)]
struct ExtractionResponse {
    request_id: String,
    results: Vec<ExtractionResult>,
    total_processing_time_ms: u64,
    total_files_processed: usize,
    successful_extractions: usize,
    failed_extractions: usize,
}

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_ms: u64,
    timestamp: String,
}

#[derive(Error, Debug)]
enum UserError {
    #[error("Internal server error: {0}")]
    InternalError(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl ResponseError for UserError {
    fn error_response(&self) -> HttpResponse {
        error!("Request failed: {}", self);
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .json(serde_json::json!({
                "error": self.to_string(),
                "error_type": match self {
                    UserError::InternalError(_) => "internal_error",
                    UserError::BadRequest(_) => "bad_request",
                }
            }))
    }

    fn status_code(&self) -> StatusCode {
        match *self {
            UserError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            UserError::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl From<std::io::Error> for UserError {
    fn from(err: std::io::Error) -> Self {
        UserError::InternalError(format!("IO error: {}", err))
    }
}

/// Health check endpoint
#[get("/health")]
#[instrument]
async fn health_check(start_time: web::Data<Instant>) -> ActixResult<HttpResponse> {
    let uptime = start_time.elapsed().as_millis() as u64;
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_ms: uptime,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    
    info!("Health check requested, uptime: {}ms", uptime);
    Ok(HttpResponse::Ok().json(response))
}

/// Parses content from a list of URLs provided in a JSON POST request.
#[post("/parse/urls")]
#[instrument(skip(extractor, req))]
async fn parse_urls_handler(
    extractor: web::Data<Arc<Extractor>>,
    req: web::Json<UrlListRequest>,
) -> Result<HttpResponse, UserError> {
    let request_id = Uuid::new_v4().to_string();
    let start_time = Instant::now();
    
    info!(
        request_id = %request_id,
        url_count = req.urls.len(),
        "Starting URL parsing request"
    );

    if req.urls.is_empty() {
        return Err(UserError::BadRequest("No URLs provided".to_string()));
    }

    if req.urls.len() > 10 {
        return Err(UserError::BadRequest("Too many URLs (max 10 allowed)".to_string()));
    }

    let mut results: Vec<ExtractionResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for (index, url_str) in req.urls.iter().enumerate() {
        let result_id = format!("{}-url-{}", request_id, index);
        let url_start_time = Instant::now();
        
        debug!(
            request_id = %request_id,
            result_id = %result_id,
            url = %url_str,
            "Processing URL"
        );
        
        match (**extractor).extract_url_to_string(url_str) {
            Ok((extracted_text, metadata)) => {
                let processing_time = url_start_time.elapsed().as_millis() as u64;
                info!(
                    request_id = %request_id,
                    result_id = %result_id,
                    url = %url_str,
                    processing_time_ms = processing_time,
                    text_length = extracted_text.len(),
                    "Successfully extracted text from URL"
                );
                
                results.push(ExtractionResult {
                    id: result_id,
                    url: Some(url_str.clone()),
                    file_name: None,
                    input_index: index,
                    extracted_text,
                    metadata: serde_json::to_value(metadata).unwrap_or_default(),
                    processing_time_ms: processing_time,
                    error: None,
                });
                successful += 1;
            }
            Err(e) => {
                let processing_time = url_start_time.elapsed().as_millis() as u64;
                warn!(
                    request_id = %request_id,
                    result_id = %result_id,
                    url = %url_str,
                    processing_time_ms = processing_time,
                    error = %e,
                    "Failed to extract text from URL"
                );
                
                results.push(ExtractionResult {
                    id: result_id,
                    url: Some(url_str.clone()),
                    file_name: None,
                    input_index: index,
                    extracted_text: String::new(),
                    metadata: serde_json::Value::Null,
                    processing_time_ms: processing_time,
                    error: Some(format!("Extraction failed: {}", e)),
                });
                failed += 1;
            }
        }
    }

    let total_time = start_time.elapsed().as_millis() as u64;
    
    info!(
        request_id = %request_id,
        total_processing_time_ms = total_time,
        successful_extractions = successful,
        failed_extractions = failed,
        "Completed URL parsing request"
    );

    Ok(HttpResponse::Ok().json(ExtractionResponse {
        request_id,
        results,
        total_processing_time_ms: total_time,
        total_files_processed: req.urls.len(),
        successful_extractions: successful,
        failed_extractions: failed,
    }))
}

/// Parses content from files provided in a multipart POST request.
#[post("/parse/files")]
#[instrument(skip(extractor, payload))]
async fn parse_files_handler(
    extractor: web::Data<Arc<Extractor>>,
    mut payload: Multipart,
) -> Result<HttpResponse, UserError> {
    let request_id = Uuid::new_v4().to_string();
    let start_time = Instant::now();
    
    info!(
        request_id = %request_id,
        "Starting file parsing request"
    );

    let mut results: Vec<ExtractionResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;
    let mut total_files = 0;
    
    let temp_dir = PathBuf::from("temp_uploads");
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| UserError::InternalError(format!("Failed to create temp directory: {}", e)))?;

    while let Some(mut field) = payload
        .try_next()
        .await
        .map_err(|e| UserError::InternalError(format!("Failed to get multipart field: {}", e)))?
    {
        let result_id = format!("{}-file-{}", request_id, total_files);
        let file_start_time = Instant::now();
        total_files += 1;
        
        let content_disposition = field.content_disposition();
        let default_filename = format!("unknown_file_{}", total_files);
        let original_filename = content_disposition
            .and_then(|cd| cd.get_filename())
            .unwrap_or(&default_filename);

        // Keep the original filename for response
        let file_name_str = original_filename.to_string();
        
        // Create a sanitized filename for the filesystem
        let sanitized_filename = sanitize_filename(original_filename);
        let safe_filename = if sanitized_filename.is_empty() {
            format!("file_{}", total_files)
        } else {
            sanitized_filename
        };
        
        let file_path = temp_dir.join(&safe_filename);
        
        debug!(
            request_id = %request_id,
            result_id = %result_id,
            file_name = %file_name_str,
            "Processing uploaded file"
        );

        let mut file = AsyncFile::create(&file_path).await.map_err(|e| {
            UserError::InternalError(format!("Failed to create temp file {:?}: {}", file_path, e))
        })?;

        let mut file_size = 0;
        while let Some(chunk) = field.try_next().await.map_err(|e| {
            UserError::InternalError(format!("Failed to read chunk from multipart field: {}", e))
        })? {
            file_size += chunk.len();
            if file_size > 50 * 1024 * 1024 { // 50MB limit
                let _ = tokio::fs::remove_file(&file_path).await;
                return Err(UserError::BadRequest("File too large (max 50MB)".to_string()));
            }
            
            file.write_all(&chunk).await.map_err(|e| {
                UserError::InternalError(format!("Failed to write chunk to file: {}", e))
            })?;
        }

        match (**extractor).extract_file_to_string(file_path.to_str().unwrap()) {
            Ok((extracted_text, metadata)) => {
                let processing_time = file_start_time.elapsed().as_millis() as u64;
                info!(
                    request_id = %request_id,
                    result_id = %result_id,
                    file_name = %file_name_str,
                    file_size = file_size,
                    processing_time_ms = processing_time,
                    text_length = extracted_text.len(),
                    "Successfully extracted text from file"
                );
                
                results.push(ExtractionResult {
                    id: result_id,
                    url: None,
                    file_name: Some(file_name_str),
                    input_index: total_files - 1, // total_files is incremented before processing
                    extracted_text,
                    metadata: serde_json::to_value(metadata).unwrap_or_default(),
                    processing_time_ms: processing_time,
                    error: None,
                });
                successful += 1;
            }
            Err(e) => {
                let processing_time = file_start_time.elapsed().as_millis() as u64;
                warn!(
                    request_id = %request_id,
                    result_id = %result_id,
                    file_name = %file_name_str,
                    file_size = file_size,
                    processing_time_ms = processing_time,
                    error = %e,
                    "Failed to extract text from file"
                );
                
                results.push(ExtractionResult {
                    id: result_id,
                    url: None,
                    file_name: Some(file_name_str),
                    input_index: total_files - 1, // total_files is incremented before processing
                    extracted_text: String::new(),
                    metadata: serde_json::Value::Null,
                    processing_time_ms: processing_time,
                    error: Some(format!("Extraction failed: {}", e)),
                });
                failed += 1;
            }
        }
        
        // Clean up temp file
        let _ = tokio::fs::remove_file(&file_path).await;
    }

    // Clean up temp directory
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    let total_time = start_time.elapsed().as_millis() as u64;
    
    info!(
        request_id = %request_id,
        total_processing_time_ms = total_time,
        total_files_processed = total_files,
        successful_extractions = successful,
        failed_extractions = failed,
        "Completed file parsing request"
    );

    Ok(HttpResponse::Ok().json(ExtractionResponse {
        request_id,
        results,
        total_processing_time_ms: total_time,
        total_files_processed: total_files,
        successful_extractions: successful,
        failed_extractions: failed,
    }))
}

// Bearer token middleware
pub struct BearerAuth {
    token: Option<String>,
}

impl BearerAuth {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }
}

impl<S, B> Transform<S, ServiceRequest> for BearerAuth
where
    S: actix_web::dev::Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixError> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixError;
    type Transform = BearerAuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(BearerAuthMiddleware {
            service,
            token: self.token.clone(),
        })
    }
}

pub struct BearerAuthMiddleware<S> {
    service: S,
    token: Option<String>,
}

impl<S, B> actix_web::dev::Service<ServiceRequest> for BearerAuthMiddleware<S>
where
    S: actix_web::dev::Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixError> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixError;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let token = self.token.clone();
        let path = req.path().to_string();
        let is_protected = path == "/parse/files" || path == "/parse/urls";
        let auth_header = req.headers().get("Authorization").cloned();
        let fut = self.service.call(req);
        Box::pin(async move {
            if let Some(token) = token {
                if is_protected {
                    match auth_header.and_then(|h| h.to_str().ok().map(|s| s.to_string())) {
                        Some(header) if header == format!("Bearer {}", token) => {
                            // OK
                        }
                        _ => {
                            return Err(actix_web::error::ErrorUnauthorized("Invalid or missing Bearer token"));
                        }
                    }
                }
            }
            fut.await
        })
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Initialize tracing with better formatting for debugging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug,actix_web=debug,actix_server=debug".into()),
        )
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .pretty()
        .init();

    let start_time = Instant::now();
    
    info!("🔧 Initializing extractor...");
    let extractor = Arc::new(create_extractor());
    info!("✅ Extractor initialized successfully");

    // Read CORS and Bearer token config from env
    let cors_origins = env::var("ACTIX_CORS_ORIGIN").unwrap_or_else(|_| "*".to_string());
    let allowed_origins: Vec<String> = cors_origins.split(',').map(|s| s.trim().to_string()).collect();
    let bearer_token = env::var("ACTIX_BEARER_TOKEN").ok();

    info!("CORS allowed origins: {:?}", allowed_origins);
    if let Some(_) = bearer_token {
        info!("Bearer token authentication enabled");
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "🚀 Extractous Document Processing API Started"
    );
    info!("🌐 Web API listening on http://0.0.0.0:8080");
    info!("📋 Available endpoints:");
    info!("  GET  /health - Health check");
    info!("  POST /parse/files - Upload files for text extraction");
    info!("  POST /parse/urls - Extract text from URLs");
    info!("---");

    HttpServer::new(move || {
        let mut cors = Cors::default();
        for origin in &allowed_origins {
            if origin == "*" {
                cors = cors.allow_any_origin();
            } else {
                cors = cors.allowed_origin(origin);
            }
        }
        cors = cors.allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec!["Content-Type", "Authorization"])
            .supports_credentials();

        App::new()
            .app_data(web::Data::new(Arc::clone(&extractor)))
            .app_data(web::Data::new(start_time))
            .app_data(web::PayloadConfig::new(52_428_800)) // 50MB limit
            .wrap(cors)
            .wrap(Logger::new("🔄 %a \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T"))
            .wrap(BearerAuth::new(bearer_token.clone()))
            .service(health_check)
            .service(parse_urls_handler)
            .service(parse_files_handler)
    })
    .bind(("0.0.0.0", 8080))?
    .workers(1) // Single worker for easier debugging
    .run()
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_app() -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        let start_time = Instant::now();
        let extractor = Arc::new(create_extractor());
        
        App::new()
            .app_data(web::Data::new(extractor))
            .app_data(web::Data::new(start_time))
            .service(health_check)
            .service(parse_urls_handler)
            .service(parse_files_handler)
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = test::init_service(create_test_app()).await;
        let req = test::TestRequest::get()
            .uri("/health")
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: HealthResponse = test::read_body_json(resp).await;
        assert_eq!(body.status, "healthy");
        assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_parse_urls_empty_request() {
        let app = test::init_service(create_test_app()).await;
        let req = test::TestRequest::post()
            .uri("/parse/urls")
            .set_json(&json!({"urls": []}))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn test_parse_urls_too_many() {
        let app = test::init_service(create_test_app()).await;
        let urls: Vec<String> = (0..15).map(|i| format!("https://example{}.com", i)).collect();
        let req = test::TestRequest::post()
            .uri("/parse/urls")
            .set_json(&json!({"urls": urls}))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn test_parse_urls_valid_request() {
        let app = test::init_service(create_test_app()).await;
        let req = test::TestRequest::post()
            .uri("/parse/urls")
            .set_json(&json!({"urls": ["https://httpbin.org/html"]}))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: ExtractionResponse = test::read_body_json(resp).await;
        assert_eq!(body.total_files_processed, 1);
        assert!(!body.request_id.is_empty());
        assert_eq!(body.results.len(), 1);
    }

    #[tokio::test]
    async fn test_parse_files_with_text_file() {
        let app = test::init_service(create_test_app()).await;
        
        // Create a temporary text file
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp_file, "Hello, World! This is a test document.").expect("Failed to write to temp file");
        let file_path = temp_file.path();
        
        // Read file content
        let file_content = std::fs::read(file_path).expect("Failed to read temp file");
        
        // Create multipart request
        let boundary = "----formdata-test-boundary";
        let body = format!(
            "--{}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\n{}\r\n--{}--\r\n",
            boundary,
            String::from_utf8_lossy(&file_content),
            boundary
        );
        
        let req = test::TestRequest::post()
            .uri("/parse/files")
            .insert_header(("content-type", format!("multipart/form-data; boundary={}", boundary)))
            .set_payload(body)
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: ExtractionResponse = test::read_body_json(resp).await;
        assert_eq!(body.total_files_processed, 1);
        assert_eq!(body.successful_extractions, 1);
        assert_eq!(body.failed_extractions, 0);
        assert!(!body.request_id.is_empty());
        
        let result = &body.results[0];
        assert!(result.file_name.as_ref().unwrap().contains("test.txt"));
        assert!(result.extracted_text.contains("Hello, World!"));
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_invalid_endpoint() {
        let app = test::init_service(create_test_app()).await;
        let req = test::TestRequest::get()
            .uri("/invalid-endpoint")
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn test_parse_files_empty_multipart() {
        let app = test::init_service(create_test_app()).await;
        
        let boundary = "----formdata-test-boundary";
        let body = format!("--{}--\r\n", boundary);
        
        let req = test::TestRequest::post()
            .uri("/parse/files")
            .insert_header(("content-type", format!("multipart/form-data; boundary={}", boundary)))
            .set_payload(body)
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: ExtractionResponse = test::read_body_json(resp).await;
        assert_eq!(body.total_files_processed, 0);
        assert_eq!(body.successful_extractions, 0);
        assert_eq!(body.failed_extractions, 0);
    }

    #[tokio::test]
    async fn test_user_error_display() {
        let internal_error = UserError::InternalError("Test error".to_string());
        assert!(internal_error.to_string().contains("Internal server error"));
        
        let bad_request = UserError::BadRequest("Bad input".to_string());
        assert!(bad_request.to_string().contains("Bad request"));
    }

    #[tokio::test]
    async fn test_user_error_status_codes() {
        assert_eq!(UserError::InternalError("test".to_string()).status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(UserError::BadRequest("test".to_string()).status_code(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("normal_file.pdf"), "normal_file.pdf");
        assert_eq!(sanitize_filename("Прайс kbcn на 2 квартал 2025 г v1.pdf"), "___kbcn____2_______2025___v1.pdf");
        assert_eq!(sanitize_filename("файл с пробелами.txt"), "____________.txt");
        assert_eq!(sanitize_filename("file with spaces.doc"), "file_with_spaces.doc");
        assert_eq!(sanitize_filename("спец!@#$%символы.xlsx"), "____.xlsx");
        assert_eq!(sanitize_filename(""), "");
    }
}
