#![no_main]
use gif::Decoder;
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;
use common::{NO, check_info, parse};

// read_info (free fn and Decoder) agree. When the full decode succeeds, it gives the same Info.
fuzz_target!(|src: &[u8]| {
    let Some((l, _, data)) = parse(src) else {
        return;
    };
    let a = gif::read_info(data, &l, &NO);
    let b = Decoder::new().read_info(data, &l, &NO).map(|i| i.clone());
    assert_eq!(a, b);
    if let Ok(i) = &a {
        check_info(i, data);
    }
    if let (Ok(i), Ok((f, _))) = (&a, gif::decode(data, &l, &NO)) {
        assert_eq!(i, &f);
    }
});
