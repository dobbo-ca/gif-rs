#![no_main]
use gif::Decoder;
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;
use common::{NO, parse};

type Res = Result<(gif::Info, Vec<u8>), gif::Error>;

// data = [a b][option byte]: the option byte's bits 2..8 place the split, `a` is the first
// `(byte >> 2) * len / 64` bytes. One Decoder decodes a, a again, then b into one reused buffer.
// Each result must equal a fresh decode (a stale byte or stale Info leaking out of reused scratch
// is a crash). The second decode of `a` must equal the first.
fuzz_target!(|src: &[u8]| {
    let Some((l, last, rest)) = parse(src) else {
        return;
    };
    let (a, b) = rest.split_at(usize::from(last >> 2) * rest.len() / 64);
    let mut dec = Decoder::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut first: Option<Res> = None;
    for (n, data) in [a, a, b].into_iter().enumerate() {
        let fresh = gif::decode(data, &l, &NO);
        // Size the buffer from the header, as a caller does.
        let need = match dec.read_info(data, &l, &NO) {
            Ok(i) => i.output_size(),
            Err(_) => 0,
        };
        // Dirty the reused buffer so unwritten bytes show up.
        buf.clear();
        buf.resize(need, 0xA5);
        let reused = dec.decode_into(data, &mut buf, &l, &NO).map(|i| i.clone());
        let got: Res = match (reused, fresh) {
            (Ok(i), Ok((fi, fo))) => {
                assert_eq!(i, fi);
                assert_eq!(buf, fo);
                Ok((i, fo))
            }
            (Err(e), Err(f)) => {
                assert_eq!(e, f);
                Err(e)
            }
            (r, f) => panic!("reuse {:?} != fresh {:?}", r.map(|_| ()), f.map(|_| ())),
        };
        match n {
            0 => first = Some(got),
            1 => assert_eq!(first.as_ref(), Some(&got)),
            _ => {}
        }
    }
});
