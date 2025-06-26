# Extractous Document Processing API

Fast document text extraction API built with Rust and [Extractous](https://github.com/yobix-ai/extractous). Supports multiple formats with OCR capabilities.

## Features

- **📄 Multi-Format**: PDF, DOCX, TXT, PNG, JPG, HTML, EML
- **🔍 OCR Support**: Tesseract integration for images and scanned PDFs
- **⚡ Fast**: Async processing with proper error handling
- **🐳 Docker Ready**: Complete containerization
- **📊 Monitoring**: Health checks and structured logging

## Quick Start

### Docker (Recommended)

```bash
# Start the service
docker-compose up -d

# Check health
curl http://localhost:8280/health

# Upload a file
curl -X POST -F "file=@document.pdf" http://localhost:8280/parse/files
```

### Local Development

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Run the API
cargo run

# Test
curl -X POST -F "file=@document.pdf" http://localhost:8080/parse/files
```

## API Endpoints

### Health Check
```bash
GET /health
```

### Process Files
```bash
POST /parse/files
Content-Type: multipart/form-data

# Upload files
curl -X POST -F "file=@document.pdf" http://localhost:8080/parse/files
```

### Process URLs
```bash
POST /parse/urls
Content-Type: application/json

{
  "urls": ["https://example.com/document.pdf"]
}
```

## Client-Server File Mapping

When uploading multiple files, the API ensures you can map results back to your original files:

### File Upload Example
```bash
curl -X POST http://localhost:8080/parse/files \
  -F "file=@document1.pdf" \
  -F "file=@document2.docx" \
  -F "file=@Прайс_файл.pdf"
```

### Response Structure
```json
{
  "request_id": "uuid-here",
  "results": [
    {
      "id": "uuid-file-0",
      "file_name": "document1.pdf",
      "input_index": 0,
      "extracted_text": "...",
      "error": null
    },
    {
      "id": "uuid-file-1", 
      "file_name": "document2.docx",
      "input_index": 1,
      "extracted_text": "...",
      "error": null
    },
    {
      "id": "uuid-file-2",
      "file_name": "Прайс_файл.pdf",
      "input_index": 2,
      "extracted_text": "...",
      "error": null
    }
  ],
  "total_files_processed": 3,
  "successful_extractions": 3,
  "failed_extractions": 0
}
```

### Mapping Results to Input Files

**Key Fields for Mapping:**
- `input_index`: 0-based index corresponding to the order files were uploaded
- `file_name`: Original filename as uploaded (preserves Unicode characters)
- `id`: Unique identifier for this specific extraction

**Client Implementation:**
```javascript
// When uploading files
const files = [file1, file2, file3]; // Your input files

// After receiving response
response.results.forEach(result => {
  const originalFile = files[result.input_index];
  console.log(`File: ${originalFile.name}`);
  console.log(`Server filename: ${result.file_name}`);
  console.log(`Extracted text: ${result.extracted_text}`);
});
```

**Handling Unicode Filenames:**
The server sanitizes filenames internally to avoid Unicode encoding issues with the Java/Tika layer, but always returns the original filename in the response. This means:
- `file_name` in response = original filename (e.g., "Прайс list на 2 квартал 2025 г v1.pdf")
- Internal processing uses ASCII-safe filename (e.g., "___list____2_______2025___v1.pdf")
- Client mapping works correctly regardless of Unicode characters

## Configuration

### Environment Variables

```bash
# Logging level
RUST_LOG=info

# Server configuration
SERVER_HOST=0.0.0.0
SERVER_PORT=8080

# OCR languages (use + to combine multiple)
TESSERACT_LANGUAGES=eng+rus

# CORS allowed origins (comma-separated, default: '*')
ACTIX_CORS_ORIGIN="*"
# Example: ACTIX_CORS_ORIGIN="https://myapp.com,https://admin.myapp.com"

# Bearer token for authentication (optional)
ACTIX_BEARER_TOKEN="your_secret_token"
# If set, /parse/files and /parse/urls require 'Authorization: Bearer <token>' header
```

### Adding OCR Languages

1. **Install language packs**:
   ```bash
   # Examples
   sudo apt-get install tesseract-ocr-fra  # French
   sudo apt-get install tesseract-ocr-deu  # German
   sudo apt-get install tesseract-ocr-spa  # Spanish
   ```

2. **Set environment variable**:
   ```bash
   export TESSERACT_LANGUAGES=eng+rus+fra+deu
   ```

3. **For Docker**, update `docker-compose.yml`:
   ```yaml
   environment:
     - TESSERACT_LANGUAGES=eng+rus+fra+deu
   ```

**Common language codes**: `eng` (English), `rus` (Russian), `fra` (French), `deu` (German), `spa` (Spanish), `ita` (Italian), `por` (Portuguese), `chi-sim` (Chinese), `jpn` (Japanese), `kor` (Korean), `ara` (Arabic)

## Supported Formats

| Format | Extensions | OCR Support |
|--------|------------|-------------|
| PDF | `.pdf` | ✅ Auto OCR for scanned docs |
| Word | `.docx` | ❌ Native text extraction |
| Text | `.txt` | ❌ Direct reading |
| Images | `.png`, `.jpg`, `.jpeg` | ✅ Full OCR |
| HTML | `.html` | ❌ Text extraction |
| Email | `.eml` | ❌ Content extraction |

## Limits

- Maximum 10 URLs per request
- Maximum 50MB per file upload
- OCR timeout: 240 seconds

## Testing

```bash
# Run tests
cargo test

# Health check
curl http://localhost:8080/health

# Upload test file
curl -X POST -F "file=@test.pdf" http://localhost:8080/parse/files
```

## Troubleshooting

**Build issues**:
```bash
# Install dependencies
sudo apt-get install build-essential tesseract-ocr tesseract-ocr-eng
```

**Check logs**:
```bash
# Docker
docker-compose logs -f

# Local
RUST_LOG=debug cargo run
```

**Performance**: Monitor memory usage during OCR processing. Scale horizontally for high load.

## Project Structure

```
├── src/
│   ├── main.rs          # HTTP server and routing
│   └── processor.rs     # Document processing
├── Dockerfile           # Container build
├── docker-compose.yml   # Service orchestration
└── README.md           # This file
```

### Contributing
1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass: `cargo test`
5. Submit a pull request
