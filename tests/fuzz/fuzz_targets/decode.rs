#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;
use common::{NO, check_info, parse};

// data = [gif stream][option byte]. Must not panic; output length must match Info, and every
// range in Info must lie inside the input.
fuzz_target!(|src: &[u8]| {
    let Some((limits, _, data)) = parse(src) else {
        return;
    };
    if let Ok((info, out)) = gif::decode(data, &limits, &NO) {
        assert_eq!(out.len(), info.output_size());
        assert!(info.extensions.len() <= limits.max_extensions);
        check_info(&info, data);
    }
});
