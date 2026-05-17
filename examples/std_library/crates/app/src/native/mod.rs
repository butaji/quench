//! Native functions for std_library example.

/// Calculate MD5 hash (simplified).
pub fn md5_hash(input: &str) -> String {
    // In real implementation, would use md5 crate
    format!("hash_{:x}", input.len())
}

/// Base64 encode a string.
pub fn base64_encode(input: &str) -> String {
    // Simplified - would use base64 crate in production
    let bytes: Vec<u8> = input.as_bytes().to_vec();
    format!("base64:{}:{} bytes", bytes.len(), input.len())
}

/// Format file size in human-readable form.
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Parse a version string like "1.2.3" into components.
pub fn parse_version(version: &str) -> (u32, u32, u32) {
    let parts: Vec<&str> = version.split('.').collect();
    let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1048576), "1.00 MB");
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("2.0.0"), (2, 0, 0));
        assert_eq!(parse_version("invalid"), (0, 0, 0));
    }
}
