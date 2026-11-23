/*
 * giforacle: CI-only giflib oracle for gif-rs parity tests (never linked into the crate).
 *
 *   build: gcc -O2 -Wall -static -o /usr/local/bin/giforacle giforacle.c -lgif
 *          (Debian bookworm, giflib 5.2.1; C11, giflib low-level API and libc only)
 *
 *   giforacle <in-dir> <out-dir>
 *       Walks <in-dir> recursively. Inputs are the regular files whose name ends in ".gif".
 *       Output base name = path relative to <in-dir> with '/' replaced by "__".
 *       Success: <name>.rgba and <name>.info. Failure: <name>.err (a token and a newline).
 *       The formats and every policy are in docs/design.md ("Composition", "Scan",
 *       "Parity output format").
 *   giforacle -gen <in-dir> <out-dir>
 *       Writes 40 deterministic mutations of every input of up to 1 MiB, named
 *       <name without .gif>.mNN.gif: truncations, bit flips, length-field edits,
 *       sub-block length edits, LZW code size edits.
 *   giforacle -bench <in-dir> [N]
 *       Per input: the median of N decodes in ns (default 5), as "name<TAB>ns" lines and a
 *       final "TOTAL<TAB>ns" line (the format of examples/bench). Inputs that fail are skipped.
 *
 * The decode follows DGifSlurp for the first frame only (same DGifGetLine calls), then scans
 * the rest of the file with the record functions, as docs/design.md says.
 * Allocations are bounded: the input is capped at 1 GiB, the frame index buffer at 1 GiB,
 * the canvas at 4 GiB of RGBA (lazy calloc: untouched pages cost nothing).
 */
#define _XOPEN_SOURCE 700
#include <ftw.h>
#include <gif_lib.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>

#define MAX_INPUT ((uint64_t)1 << 30)
#define MAX_FRAME_PIXELS ((uint64_t)1 << 30)
#define MAX_CANVAS_PIXELS ((uint64_t)1 << 32)
/* examples/oracle.rs mirrors this: more than 2^30 canvas pixels is "limit". */
#define MAX_CANVAS_BYTES ((uint64_t)1 << 32)
#define MAX_EXTENSIONS 4096
#define MAX_GEN_INPUT ((size_t)1 << 18)
#define GEN_PER_FILE 40

/* ---- memory input ---- */

struct mem {
    const uint8_t *data;
    size_t size, pos;
};

static int read_mem(GifFileType *g, GifByteType *out, int len)
{
    struct mem *m = (struct mem *)g->UserData;
    size_t want = len > 0 ? (size_t)len : 0;
    size_t left = m->size - m->pos;
    if (want > left)
        want = left;
    memcpy(out, m->data + m->pos, want);
    m->pos += want;
    return (int)want;
}

/* ---- growable byte buffer ---- */

struct buf {
    uint8_t *p;
    size_t len, cap;
    int oom;
};

static void bput(struct buf *b, const void *s, size_t n)
{
    if (b->oom)
        return;
    if (n > ((size_t)1 << 31) - b->len) {
        b->oom = 1;
        return;
    }
    if (b->len + n > b->cap) {
        size_t cap = b->cap ? b->cap : 256;
        while (cap < b->len + n)
            cap *= 2;
        uint8_t *np = realloc(b->p, cap);
        if (!np) {
            b->oom = 1;
            return;
        }
        b->p = np;
        b->cap = cap;
    }
    if (n)
        memcpy(b->p + b->len, s, n);
    b->len += n;
}

static void bstr(struct buf *b, const char *s)
{
    bput(b, s, strlen(s));
}

static void bhex(struct buf *b, const uint8_t *s, size_t n)
{
    static const char d[] = "0123456789abcdef";
    for (size_t i = 0; i < n; i++) {
        char c[2] = {d[s[i] >> 4], d[s[i] & 15]};
        bput(b, c, 2);
    }
}

/* ---- error tokens (docs/design.md, "Errors") ---- */

static const char *token(int code)
{
    switch (code) {
    case D_GIF_ERR_READ_FAILED: return "read-failed";
    case D_GIF_ERR_NOT_GIF_FILE: return "not-gif";
    case D_GIF_ERR_NO_SCRN_DSCR: return "no-scrn-dscr";
    case D_GIF_ERR_WRONG_RECORD: return "wrong-record";
    case D_GIF_ERR_DATA_TOO_BIG: return "data-too-big";
    case D_GIF_ERR_NOT_ENOUGH_MEM: return "oom";
    case D_GIF_ERR_IMAGE_DEFECT: return "image-defect";
    case D_GIF_ERR_EOF_TOO_SOON: return "eof-too-soon";
    default: return "read-failed"; /* code 0: giflib failed without setting one */
    }
}

/* ---- decode ---- */

struct result {
    const char *err; /* NULL on success */
    uint8_t *rgba;
    size_t rgba_len;
    struct buf info;
};

/*
 * Reads the rest of an extension block (the label and all sub-blocks) into raw: the length
 * bytes, the payloads and the 0 terminator. If the block is a GCE whose first sub-block has at
 * least 4 bytes, copies those 4 bytes to gce and sets *is_gce. Returns 0, or -1 on a giflib
 * error (the code is in g->Error).
 */
static int read_ext(GifFileType *g, struct buf *raw, int *label, int *is_gce, uint8_t gce[4])
{
    GifByteType *blk;
    int first = 1;
    raw->len = 0;
    *is_gce = 0;
    if (DGifGetExtension(g, label, &blk) == GIF_ERROR)
        return -1;
    while (blk != NULL) {
        if (first && *label == GRAPHICS_EXT_FUNC_CODE && blk[0] >= 4) {
            memcpy(gce, blk + 1, 4);
            *is_gce = 1;
        }
        first = 0;
        bput(raw, blk, (size_t)blk[0] + 1);
        if (DGifGetExtensionNext(g, &blk) == GIF_ERROR)
            return -1;
    }
    bput(raw, "\0", 1);
    return 0;
}

static void add_ext(struct buf *exts, unsigned *nrec, unsigned frame, int label, const struct buf *raw)
{
    char head[64];
    if (*nrec >= MAX_EXTENSIONS)
        return;
    (*nrec)++;
    snprintf(head, sizeof head, "ext %u %02x %zu ", frame, (unsigned)label & 0xff, raw->len);
    bstr(exts, head);
    bhex(exts, raw->p, raw->len);
    bstr(exts, "\n");
}

static void fail(struct result *r, const char *tok)
{
    r->err = tok;
}

/* Decodes one file. want_info = 0 skips the text (bench). */
static void decode_file(const uint8_t *data, size_t n, int want_info, struct result *r)
{
    struct mem m = {data, n, 0};
    int err = 0;
    GifFileType *g = DGifOpen(&m, read_mem, &err);
    GifRecordType rt;
    struct buf raw = {0}, exts = {0};
    unsigned nrec = 0, desc = 0;
    uint8_t *idx = NULL;
    GifColorType lt[256];
    int lcount = 0, ccount = 0;
    int is_gce = 0, label = 0;
    uint8_t gce[4] = {0}, cur[4] = {0};
    int have_gce = 0;

    memset(r, 0, sizeof *r);
    if (!g) {
        fail(r, token(err));
        return;
    }

    /* Records up to the first image descriptor. */
    for (;;) {
        if (DGifGetRecordType(g, &rt) == GIF_ERROR) {
            fail(r, token(g->Error));
            goto done;
        }
        if (rt == TERMINATE_RECORD_TYPE) {
            fail(r, "no-image");
            goto done;
        }
        if (rt != EXTENSION_RECORD_TYPE)
            break;
        if (read_ext(g, &raw, &label, &is_gce, cur) < 0) {
            fail(r, token(g->Error));
            goto done;
        }
        if (is_gce) {
            memcpy(gce, cur, 4); /* the last valid GCE wins */
            have_gce = 1;
        }
        if (want_info)
            add_ext(&exts, &nrec, 0, label, &raw);
    }

    /* The first image descriptor. The packed byte is 8 bytes after the 0x2C, and the LZW code
     * size is the last byte the call read: giflib does not keep either. */
    size_t pos0 = m.pos;
    if (DGifGetImageDesc(g) == GIF_ERROR) {
        fail(r, token(g->Error));
        goto done;
    }
    desc = 1;
    int left = g->Image.Left, top = g->Image.Top, w = g->Image.Width, h = g->Image.Height;
    int interlace = g->Image.Interlace;
    unsigned fflags = data[pos0 + 8], mincode = data[m.pos - 1];
    if (g->Image.ColorMap) {
        lcount = g->Image.ColorMap->ColorCount;
        memcpy(lt, g->Image.ColorMap->Colors, (size_t)lcount * sizeof lt[0]);
    }
    if (g->SColorMap)
        ccount = g->SColorMap->ColorCount;

    /* Canvas and limits, before any big allocation. */
    uint64_t cw = (uint64_t)g->SWidth, ch = (uint64_t)g->SHeight;
    if ((uint64_t)left + (uint64_t)w > cw)
        cw = (uint64_t)left + (uint64_t)w;
    if ((uint64_t)top + (uint64_t)h > ch)
        ch = (uint64_t)top + (uint64_t)h;
    uint64_t cpx = cw * ch, fpx = (uint64_t)w * (uint64_t)h;
    if (cpx > MAX_CANVAS_PIXELS || cpx * 4 > MAX_CANVAS_BYTES || fpx > MAX_FRAME_PIXELS) {
        fail(r, "limit");
        goto done;
    }
    /* DGifSlurp: an empty frame is GIF_ERROR with no code set, before any DGifGetLine call. */
    if (w == 0 || h == 0) {
        fail(r, "read-failed");
        goto done;
    }
    idx = calloc(fpx ? (size_t)fpx : 1, 1);
    if (!idx) {
        fail(r, "limit");
        goto done;
    }

    /* The first frame, with DGifSlurp's DGifGetLine calls. */
    if (interlace) {
        static const int off[4] = {0, 4, 2, 1}, jump[4] = {8, 8, 4, 2};
        for (int i = 0; i < 4; i++)
            for (int j = off[i]; j < h; j += jump[i])
                if (DGifGetLine(g, idx + (size_t)j * (size_t)w, w) == GIF_ERROR) {
                    fail(r, token(g->Error));
                    goto done;
                }
    } else if (DGifGetLine(g, idx, (int)fpx) == GIF_ERROR) {
        fail(r, token(g->Error));
        goto done;
    }
    if (!lcount && !ccount) {
        fail(r, "no-color-table");
        goto done;
    }

    /* Compose on a canvas that starts as transparent black. */
    r->rgba_len = (size_t)(cpx * 4);
    r->rgba = calloc(r->rgba_len ? r->rgba_len : 1, 1);
    if (!r->rgba) {
        fail(r, "limit");
        goto done;
    }
    const GifColorType *tab = lcount ? lt : g->SColorMap->Colors;
    int tcount = lcount ? lcount : ccount;
    int tflag = have_gce && (gce[0] & 1);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) {
            unsigned v = idx[(size_t)y * (size_t)w + (size_t)x];
            uint8_t *px = r->rgba + (((size_t)(top + y)) * (size_t)cw + (size_t)(left + x)) * 4;
            if (tflag && v == gce[3])
                continue;
            if ((int)v < tcount) {
                px[0] = tab[v].Red;
                px[1] = tab[v].Green;
                px[2] = tab[v].Blue;
            }
            px[3] = 255;
        }

    /* Scan the rest. An error here ends the scan, never the file. */
    unsigned frames = 1;
    int complete = 0;
    for (;;) {
        if (DGifGetRecordType(g, &rt) == GIF_ERROR)
            break;
        if (rt == TERMINATE_RECORD_TYPE) {
            complete = 1;
            break;
        }
        if (rt == IMAGE_DESC_RECORD_TYPE) {
            int cs;
            GifByteType *blk;
            if (DGifGetImageDesc(g) == GIF_ERROR || DGifGetCode(g, &cs, &blk) == GIF_ERROR)
                break;
            desc++;
            while (blk != NULL)
                if (DGifGetCodeNext(g, &blk) == GIF_ERROR)
                    goto scan_done;
            frames++;
        } else if (rt == EXTENSION_RECORD_TYPE) {
            if (read_ext(g, &raw, &label, &is_gce, cur) < 0)
                break;
            if (want_info)
                add_ext(&exts, &nrec, desc, label, &raw);
        } else {
            break;
        }
    }
scan_done:

    if (want_info) {
        char line[256];
        struct buf *b = &r->info;
        snprintf(line, sizeof line, "canvas %llu %llu\n", (unsigned long long)cw, (unsigned long long)ch);
        bstr(b, line);
        snprintf(line, sizeof line, "screen %d %d %u %u %u\n", g->SWidth, g->SHeight, (unsigned)data[10],
                 (unsigned)data[11], (unsigned)data[12]);
        bstr(b, line);
        snprintf(line, sizeof line, "frame %d %d %d %d %u %u\n", left, top, w, h, fflags, mincode);
        bstr(b, line);
        if (have_gce) {
            char t[16] = "-";
            if (gce[0] & 1)
                snprintf(t, sizeof t, "%u", (unsigned)gce[3]);
            snprintf(line, sizeof line, "control %u %u %u %s\n", (unsigned)(gce[0] >> 2) & 7,
                     (unsigned)(gce[0] >> 1) & 1, (unsigned)gce[1] | ((unsigned)gce[2] << 8), t);
            bstr(b, line);
        } else {
            bstr(b, "control -\n");
        }
        snprintf(line, sizeof line, "frames %u %d\n", frames, complete);
        bstr(b, line);
        bput(b, exts.p, exts.len);
        if (b->oom)
            fail(r, "oom");
    }

done:
    free(idx);
    free(raw.p);
    free(exts.p);
    DGifCloseFile(g, NULL);
    if (r->err) {
        free(r->rgba);
        r->rgba = NULL;
        r->rgba_len = 0;
    }
}

static void result_free(struct result *r)
{
    free(r->rgba);
    free(r->info.p);
    memset(r, 0, sizeof *r);
}

/* ---- files ---- */

static int write_file(const char *dir, const char *name, const char *ext, const void *p, size_t n)
{
    char path[8300];
    if (snprintf(path, sizeof path, "%s/%s.%s", dir, name, ext) >= (int)sizeof path)
        return -1;
    FILE *f = fopen(path, "wb");
    if (!f)
        return -1;
    int ok = n == 0 || fwrite(p, 1, n, f) == n;
    return (fclose(f) == 0 && ok) ? 0 : -1;
}

static int mode; /* 0 decode, 1 gen, 2 bench */
static unsigned long reps = 5;
static const char *in_root, *out_dir;
static int failures;
static uint64_t bench_total;

static uint64_t now_ns(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec;
}

static int cmp_u64(const void *a, const void *b)
{
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return x < y ? -1 : x > y;
}

/* ---- mutations ---- */

static uint64_t rng = 1;

static uint64_t xr(void)
{
    rng ^= rng << 13;
    rng ^= rng >> 7;
    rng ^= rng << 17;
    return rng;
}

/* Offset of the first 0x2C record, or -1. */
static long find_desc(const uint8_t *d, size_t n)
{
    size_t p = 13;
    if (n < 13)
        return -1;
    if (d[10] & 0x80)
        p += (size_t)3 << ((d[10] & 7) + 1);
    while (p < n) {
        if (d[p] == 0x2c)
            return (long)p;
        if (d[p] != 0x21)
            return -1;
        p += 2;
        while (p < n && d[p] != 0)
            p += (size_t)d[p] + 1;
        p++;
    }
    return -1;
}

/* Offset of the LZW code size byte of the first image, or -1. */
static long find_code(const uint8_t *d, size_t n, long desc)
{
    if (desc < 0 || (size_t)desc + 10 > n)
        return -1;
    size_t p = (size_t)desc + 10;
    if (d[desc + 9] & 0x80)
        p += (size_t)3 << ((d[desc + 9] & 7) + 1);
    return p < n ? (long)p : -1;
}

static void put_word(uint8_t *w, size_t at, size_t n, unsigned v)
{
    if (at + 2 <= n) {
        w[at] = (uint8_t)v;
        w[at + 1] = (uint8_t)(v >> 8);
    }
}

/* Flips one bit, not in a canvas size field (a huge canvas only makes big output files). */
static void flip(uint8_t *w, size_t n, long desc)
{
    size_t pos = 0;
    for (int t = 0; t < 16; t++) {
        pos = (size_t)(xr() % n);
        int dim = (pos >= 6 && pos < 10) || (desc >= 0 && pos >= (size_t)desc + 5 && pos < (size_t)desc + 9);
        if (!dim)
            break;
    }
    w[pos] ^= (uint8_t)(1u << (xr() % 8));
}

static void gen_one(const uint8_t *d, size_t n, const char *stem)
{
    long desc = find_desc(d, n), code = find_code(d, n, desc);
    uint8_t *w = malloc(n + 1);
    static size_t blocks[8192];
    char name[8200];
    if (!w) {
        failures++;
        return;
    }
    for (int k = 0; k < GEN_PER_FILE; k++) {
        int kind = k / 8, j = k % 8;
        size_t len = n;
        memcpy(w, d, n);
        if (kind == 0) {
            len = j < 4 ? n * (size_t)(j + 1) / 5 : (size_t)(xr() % (n + 1));
        } else if (kind == 2) {
            unsigned v = (unsigned)(xr() % 301);
            switch (xr() % 4) {
            case 0: put_word(w, 6, n, v); break;
            case 1: put_word(w, 8, n, v); break;
            case 2: if (desc >= 0) put_word(w, (size_t)desc + 5, n, v); else put_word(w, 6, n, v); break;
            default: if (desc >= 0) put_word(w, (size_t)desc + 7, n, v); else put_word(w, 8, n, v); break;
            }
        } else if (kind == 3 && code >= 0) {
            size_t cnt = 0, q = (size_t)code + 1;
            while (q < n && cnt < 8192) {
                blocks[cnt++] = q;
                if (d[q] == 0)
                    break;
                q += (size_t)d[q] + 1;
            }
            if (cnt) {
                size_t at = blocks[xr() % cnt];
                w[at] = (j % 3 == 0) ? 0 : (uint8_t)xr();
            }
        } else if (kind == 4 && code >= 0) {
            w[code] = (uint8_t)(xr() % 13);
        } else {
            flip(w, n, desc);
            if (j % 2)
                flip(w, n, desc);
        }
        if (snprintf(name, sizeof name, "%s.m%02d", stem, k) >= (int)sizeof name || write_file(out_dir, name, "gif", w, len)) {
            failures++;
        }
    }
    free(w);
}

/* ---- walk ---- */

static int visit(const char *path, const struct stat *sb, int type, struct FTW *ftw)
{
    (void)ftw;
    size_t pl = strlen(path);
    if (type != FTW_F || pl < 4 || strcmp(path + pl - 4, ".gif") != 0)
        return 0;
    const char *rel = path + strlen(in_root);
    while (*rel == '/')
        rel++;
    char name[8200];
    size_t o = 0;
    for (const char *s = rel; *s && o < sizeof name - 3; s++) {
        if (*s == '/')
            name[o++] = '_', name[o++] = '_';
        else
            name[o++] = *s;
    }
    name[o] = 0;

    if ((uint64_t)sb->st_size > MAX_INPUT) {
        fprintf(stderr, "too big, skipped: %s\n", path);
        return 0;
    }
    size_t n = (size_t)sb->st_size;
    uint8_t *data = malloc(n + 1);
    FILE *f = fopen(path, "rb");
    if (!data || !f || fread(data, 1, n, f) != n) {
        fprintf(stderr, "cannot read %s\n", path);
        failures++;
        if (f)
            fclose(f);
        free(data);
        return 0;
    }
    fclose(f);

    if (mode == 1) {
        if (n > 0 && n <= MAX_GEN_INPUT) {
            size_t sl = strlen(name);
            uint64_t h = 1469598103934665603ull; /* FNV-1a of the name: the seed */
            for (size_t i = 0; i < sl; i++)
                h = (h ^ (uint8_t)name[i]) * 1099511628211ull;
            rng = h ? h : 1;
            name[sl - 4] = 0; /* strip .gif */
            gen_one(data, n, name);
        }
    } else if (mode == 2) {
        struct result r;
        decode_file(data, n, 0, &r);
        int ok = r.err == NULL;
        result_free(&r);
        if (ok) {
            uint64_t *t = malloc(reps * sizeof *t);
            if (t) {
                for (unsigned long i = 0; i < reps; i++) {
                    uint64_t a = now_ns();
                    decode_file(data, n, 0, &r);
                    t[i] = now_ns() - a;
                    result_free(&r);
                }
                qsort(t, reps, sizeof *t, cmp_u64);
                printf("%s\t%llu\n", name, (unsigned long long)t[reps / 2]);
                bench_total += t[reps / 2];
                free(t);
            }
        }
    } else {
        struct result r;
        int bad = 0;
        decode_file(data, n, 1, &r);
        if (r.err) {
            char line[64];
            snprintf(line, sizeof line, "%s\n", r.err);
            bad = write_file(out_dir, name, "err", line, strlen(line));
        } else {
            bad |= write_file(out_dir, name, "rgba", r.rgba, r.rgba_len);
            bad |= write_file(out_dir, name, "info", r.info.p, r.info.len);
        }
        result_free(&r);
        if (bad)
            failures++;
    }
    free(data);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 4 && !strcmp(argv[1], "-gen")) {
        mode = 1;
        in_root = argv[2];
        out_dir = argv[3];
    } else if ((argc == 3 || argc == 4) && !strcmp(argv[1], "-bench")) {
        mode = 2;
        in_root = argv[2];
        if (argc == 4) {
            reps = strtoul(argv[3], NULL, 10);
            if (reps < 1)
                reps = 1;
            if (reps > 1001)
                reps = 1001;
        }
    } else if (argc == 3) {
        in_root = argv[1];
        out_dir = argv[2];
    } else {
        fprintf(stderr, "usage: giforacle <in-dir> <out-dir> | -gen <in-dir> <out-dir> | -bench <in-dir> [N]\n");
        return 2;
    }
    if (nftw(in_root, visit, 32, FTW_PHYS) != 0) {
        fprintf(stderr, "walk failed: %s\n", in_root);
        return 1;
    }
    if (mode == 2)
        printf("TOTAL\t%llu\n", (unsigned long long)bench_total);
    return failures ? 1 : 0;
}
