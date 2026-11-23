#![no_main]
use gif::Decoder;
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;
use common::{NO, limits, parse};

type Res = Result<(gif::Info, Vec<u8>), gif::Error>;

// data = [rest][option byte]: the option byte's bits 2..8 place the cut, `a` is the first
// `(byte >> 2) * len / 64` bytes of `rest` (half of it if that is 0, so a raw seed with a 0x00
// option byte still gets a truncated `a`). One Decoder decodes a, a again, then the whole of
// `rest` into one reused buffer. `a` is a truncated prefix: it usually fails partway and leaves
// stale LZW state, and the third call decodes the full input on that warm decoder. Each result
// must equal a fresh decode (a stale byte or stale Info leaking out of reused scratch is a
// crash). The second decode of `a` must equal the first. `read_info` on the warm decoder must
// equal a fresh `read_info`. The limits are never tight here: `decode` and `decode_into` may
// record different extension lists under a tight `max_alloc`.
fuzz_target!(|src: &[u8]| {
    let Some((l, last, rest)) = parse(src) else {
        return;
    };
    let l = limits(l.max_extensions, false);
    let cut = match usize::from(last >> 2) {
        0 => 32,
        k => k,
    };
    let (a, _) = rest.split_at(cut * rest.len() / 64);
    let mut dec = Decoder::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut first: Option<Res> = None;
    for (n, data) in [a, a, rest].into_iter().enumerate() {
        let fresh = gif::decode(data, &l, &NO);
        // Size the buffer from the header, as a caller does.
        let info = dec.read_info(data, &l, &NO).map(|i| i.clone());
        assert_eq!(info, gif::read_info(data, &l, &NO));
        let need = match &info {
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
