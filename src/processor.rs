use extractous::{Extractor, PdfOcrStrategy, PdfParserConfig, TesseractOcrConfig};
use std::env;

/// Create a configured extractor instance
/// 
/// ## Adding New OCR Languages
/// 
/// To add support for additional languages:
/// 
/// 1. **Install language packs** (required for each deployment):
///    ```bash
///    # Debian/Ubuntu
///    sudo apt-get install tesseract-ocr-[lang]
///    
///    # Examples:
///    sudo apt-get install tesseract-ocr-fra  # French
///    sudo apt-get install tesseract-ocr-deu  # German  
///    sudo apt-get install tesseract-ocr-spa  # Spanish
///    sudo apt-get install tesseract-ocr-chi-sim  # Chinese Simplified
///    sudo apt-get install tesseract-ocr-jpn  # Japanese
///    ```
/// 
/// 2. **Update environment variable**:
///    ```bash
///    # Single language
///    export TESSERACT_LANGUAGES=eng
///    
///    # Multiple languages (use + separator)
///    export TESSERACT_LANGUAGES=eng+rus+fra+deu
///    ```
/// 
/// 3. **For Docker deployment**, update docker-compose.yml:
///    ```yaml
///    environment:
///      - TESSERACT_LANGUAGES=eng+rus+fra+deu
///    ```
/// 
/// 4. **Common language codes**:
///    - `eng` - English
///    - `rus` - Russian  
///    - `fra` - French
///    - `deu` - German
///    - `spa` - Spanish
///    - `ita` - Italian
///    - `por` - Portuguese
///    - `chi-sim` - Chinese Simplified
///    - `chi-tra` - Chinese Traditional
///    - `jpn` - Japanese
///    - `kor` - Korean
///    - `ara` - Arabic
///    - `hin` - Hindi
/// 
/// See: https://github.com/tesseract-ocr/tessdoc/blob/main/Data-Files-in-different-versions.md
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