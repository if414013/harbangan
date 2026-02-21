//! Converters for Antigravity (Cloud Code) request/response formats.
//!
//! Converts between Anthropic Messages API format and Google Generative AI
//! format used by the Cloud Code API.

pub mod anthropic_to_google;
pub mod content_converter;
pub mod google_to_anthropic;
pub mod google_to_openai;
pub mod schema_sanitizer;
