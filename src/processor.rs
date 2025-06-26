use extractous::{Extractor, PdfOcrStrategy, PdfParserConfig, TesseractOcrConfig};
use std::env;

pub fn create_extractor() -> Extractor {
    // Get Tesseract languages from environment variable, default to "eng+rus"
    let tesseract_languages = env::var("TESSERACT_LANGUAGES")
        .unwrap_or_else(|_| "eng+rus".to_string());
    
    println!("🔤 Using Tesseract languages: {}", tesseract_languages);
    
    Extractor::new()
        .set_ocr_config(
            TesseractOcrConfig::new()
                .set_language(&tesseract_languages)
                .set_enable_image_preprocessing(true)
                .set_timeout_seconds(240),
        )
        .set_pdf_config(PdfParserConfig::new().set_ocr_strategy(PdfOcrStrategy::AUTO))
} 