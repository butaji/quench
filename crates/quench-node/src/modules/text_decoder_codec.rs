//! WHATWG label resolution and stateless decoding shared by Node adapters.

pub(crate) fn decode_bytes(encoding: &str, bytes: &[u8], fatal: bool) -> Result<String, ()> {
    #[cfg(quench_icu)]
    if needs_icu(encoding) {
        // Node follows the Encoding Standard's GBK decoder alias while keeping
        // the public canonical label as gbk.
        let converter = if encoding == "gbk" {
            "gb18030"
        } else {
            encoding
        };
        return super::text_decoder_icu::decode(converter, bytes, fatal);
    }
    let codec = encoding_rs::Encoding::for_label_no_replacement(encoding.as_bytes()).ok_or(())?;
    // TextDecoder handles its matching BOM through ignoreBOM. Codec sniffing
    // would incorrectly switch an explicitly requested UTF-8/UTF-16 encoding.
    if fatal {
        codec
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|text| text.into_owned())
            .ok_or(())
    } else {
        Ok(codec.decode_without_bom_handling(bytes).0.into_owned())
    }
}

pub(crate) fn canonical_encoding(label: &str) -> Option<String> {
    encoding_rs::Encoding::for_label(label.as_bytes())
        .map(|encoding| encoding.name().to_ascii_lowercase())
        .filter(|encoding| cfg!(quench_icu) || !needs_icu(encoding))
}

fn needs_icu(encoding: &str) -> bool {
    encoding_rs::Encoding::for_label(encoding.as_bytes()).is_some_and(|codec| {
        !codec.is_single_byte()
            && !matches!(encoding, "utf-8" | "utf-16le" | "utf-16be" | "replacement")
    })
}
