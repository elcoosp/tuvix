/*
 * tuvix.h — the C ABI embedding boundary of the tuvix terminal UI framework.
 *
 * A host drives a tuvix buffer through an opaque handle: apply mutation
 * packets (the `tuvix-core` `embed` protocol, version 1), read the screen
 * back as a snapshot packet (version 2) or as canonical ANSI. One apply
 * call carries a whole batch of mutations.
 *
 * Ownership contract: a handle returned by tvx_buffer_new stays valid
 * until exactly one tvx_buffer_free; every use after free is undefined
 * behavior. Handles are not thread-safe — use each from one thread at a
 * time. Packet `data` pointers need only be valid for the duration of
 * the call and for `len` bytes.
 *
 * Result codes (i32 returns): TVX_OK, TVX_ERR_NULL, TVX_ERR_PACKET.
 * Output functions (i64 returns) deliver byte counts: non-negative for
 * bytes written, -needed when the capacity was too small (retry with
 * cap = needed).
 */

#ifndef TUVIX_H
#define TUVIX_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Result codes. */
#define TVX_OK          0
#define TVX_ERR_NULL   (-1)  /* a required pointer argument was null */
#define TVX_ERR_PACKET  1    /* packet rejected; see err_kind output  */

/* Packet error kind codes (the `err_kind` output of tvx_buffer_apply). */
#define TVX_ERR_KIND_BAD_MAGIC           1
#define TVX_ERR_KIND_UNKNOWN_VERSION     2
#define TVX_ERR_KIND_UNEXPECTED_END      3
#define TVX_ERR_KIND_UNKNOWN_OP          4
#define TVX_ERR_KIND_UNKNOWN_COLOR       5
#define TVX_ERR_KIND_INVALID_UTF8        6
#define TVX_ERR_KIND_CONTROL_CHARACTER   7
#define TVX_ERR_KIND_TRAILING_BYTES      8
#define TVX_ERR_KIND_RESERVED_BITS       9
#define TVX_ERR_KIND_CELL_OUT_OF_RANGE  10

/* Opaque buffer handle. */
typedef struct TvxBuffer TvxBuffer;

/* The ABI version, packed (major << 16) | (minor << 8) | patch. */
uint32_t tvx_version(void);

/* The crate's semantic version as a static string. */
const char *tvx_string_version(void);

/* Lifecycle. */
TvxBuffer *tvx_buffer_new(uint16_t width, uint16_t height);
void tvx_buffer_free(TvxBuffer *handle);

/*
 * Applies one mutation packet, all-or-nothing.
 *
 * On success returns TVX_OK and sets *consumed to len (when not NULL).
 * On a rejected packet returns TVX_ERR_PACKET, sets *err_kind to one of
 * the TVX_ERR_KIND_* codes and *err_offset to the offending byte (both
 * may be NULL); nothing is applied and *consumed receives the error
 * offset — the packet is valid up to there.
 */
int32_t tvx_buffer_apply(
    TvxBuffer *handle,
    const uint8_t *data,
    size_t len,
    size_t *consumed,
    int32_t *err_kind,
    size_t *err_offset
);

/* Resizes the buffer, losing content when the area changes. */
int32_t tvx_buffer_resize(TvxBuffer *handle, uint16_t width, uint16_t height);

/* Writes the buffer's width and height (either pointer may be NULL). */
int32_t tvx_buffer_size(
    TvxBuffer *handle,
    uint16_t *width,
    uint16_t *height
);

/*
 * Encodes the visible grid as a frame snapshot packet. Returns the byte
 * count written, or -needed when cap was too small (out may be NULL when
 * cap is zero).
 */
int64_t tvx_buffer_snapshot(
    TvxBuffer *handle,
    uint8_t *out,
    size_t cap
);

/*
 * Renders the visible grid as the canonical ANSI snapshot (the M4 preview
 * encoding), with the same capacity negotiation as tvx_buffer_snapshot.
 */
int64_t tvx_buffer_render_ansi(
    TvxBuffer *handle,
    uint8_t *out,
    size_t cap
);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TUVIX_H */
