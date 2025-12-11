/*
 * nghttp3.h - nghttp3 compatible C API header for NexaOS nh3 library
 *
 * This header provides the C ABI interface for the nh3 HTTP/3 library.
 * It is compatible with the nghttp3 library API.
 *
 * Copyright (c) 2024 NexaOS Project
 * SPDX-License-Identifier: MIT
 */

#ifndef NGHTTP3_H
#define NGHTTP3_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>
#include <stddef.h>

/*
 * Version Information
 */
#define NGHTTP3_VERSION "1.0.0"
#define NGHTTP3_VERSION_NUM 0x010000

/*
 * Error Codes
 */
typedef enum {
    NGHTTP3_NO_ERROR = 0,
    NGHTTP3_ERR_INVALID_ARGUMENT = -101,
    NGHTTP3_ERR_NOBUF = -102,
    NGHTTP3_ERR_INVALID_STATE = -103,
    NGHTTP3_ERR_WOULDBLOCK = -104,
    NGHTTP3_ERR_STREAM_IN_USE = -105,
    NGHTTP3_ERR_PUSH_ID_BLOCKED = -106,
    NGHTTP3_ERR_MALFORMED_HTTP_HEADER = -107,
    NGHTTP3_ERR_REMOVE_HTTP_HEADER = -108,
    NGHTTP3_ERR_MALFORMED_HTTP_MESSAGING = -109,
    NGHTTP3_ERR_QPACK_FATAL = -110,
    NGHTTP3_ERR_QPACK_HEADER_TOO_LARGE = -111,
    NGHTTP3_ERR_IGNORE_STREAM = -112,
    NGHTTP3_ERR_H3_FRAME_UNEXPECTED = -113,
    NGHTTP3_ERR_H3_FRAME_ERROR = -114,
    NGHTTP3_ERR_H3_MISSING_SETTINGS = -115,
    NGHTTP3_ERR_H3_INTERNAL_ERROR = -116,
    NGHTTP3_ERR_H3_CLOSED_CRITICAL_STREAM = -117,
    NGHTTP3_ERR_H3_GENERAL_PROTOCOL_ERROR = -118,
    NGHTTP3_ERR_H3_ID_ERROR = -119,
    NGHTTP3_ERR_H3_SETTINGS_ERROR = -120,
    NGHTTP3_ERR_H3_STREAM_CREATION_ERROR = -121,
    NGHTTP3_ERR_FATAL = -501,
    NGHTTP3_ERR_NOMEM = -502,
    NGHTTP3_ERR_CALLBACK_FAILURE = -503,
} nghttp3_error;

/*
 * Type Definitions
 */
typedef int64_t nghttp3_stream_id;
typedef uint64_t nghttp3_push_id;

/*
 * Forward Declarations
 */
typedef struct nghttp3_conn nghttp3_conn;
typedef struct nghttp3_callbacks nghttp3_callbacks;
typedef struct nghttp3_settings nghttp3_settings;
typedef struct nghttp3_mem nghttp3_mem;

/*
 * Name-Value Pair (Header)
 */
typedef struct {
    uint8_t *name;
    uint8_t *value;
    size_t namelen;
    size_t valuelen;
    uint8_t flags;
} nghttp3_nv;

#define NGHTTP3_NV_FLAG_NONE 0x00
#define NGHTTP3_NV_FLAG_NEVER_INDEX 0x01
#define NGHTTP3_NV_FLAG_NO_COPY_NAME 0x02
#define NGHTTP3_NV_FLAG_NO_COPY_VALUE 0x04

/*
 * Priority
 */
typedef struct {
    uint8_t urgency;
    uint8_t inc;
} nghttp3_pri;

#define NGHTTP3_DEFAULT_URGENCY 3
#define NGHTTP3_URGENCY_HIGH 0
#define NGHTTP3_URGENCY_LOW 7

/*
 * Reference-Counted Buffer
 */
typedef struct {
    uint8_t *base;
    size_t len;
} nghttp3_rcbuf;

/*
 * Vector (iovec-like)
 */
typedef struct {
    uint8_t *base;
    size_t len;
} nghttp3_vec;

/*
 * Data Reader (for request/response body)
 */
typedef ssize_t (*nghttp3_read_data_callback)(
    nghttp3_conn *conn,
    int64_t stream_id,
    nghttp3_vec *vec,
    size_t veccnt,
    uint32_t *pflags,
    void *user_data,
    void *stream_user_data
);

typedef struct {
    nghttp3_read_data_callback read_data;
} nghttp3_data_reader;

#define NGHTTP3_DATA_FLAG_NONE 0x00
#define NGHTTP3_DATA_FLAG_EOF 0x01
#define NGHTTP3_DATA_FLAG_NO_END_STREAM 0x02

/*
 * Settings
 */
struct nghttp3_settings {
    uint64_t max_field_section_size;
    uint64_t qpack_max_dtable_capacity;
    uint64_t qpack_blocked_streams;
    uint8_t enable_connect_protocol;
    uint8_t h3_datagram;
};

/*
 * Version Info
 */
typedef struct {
    int age;
    int version_num;
    const char *version_str;
} nghttp3_info;

/*
 * Callback Types
 */
typedef int (*nghttp3_acked_stream_data)(
    nghttp3_conn *conn,
    int64_t stream_id,
    uint64_t datalen,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_stream_close)(
    nghttp3_conn *conn,
    int64_t stream_id,
    uint64_t app_error_code,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_recv_data)(
    nghttp3_conn *conn,
    int64_t stream_id,
    const uint8_t *data,
    size_t datalen,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_deferred_consume)(
    nghttp3_conn *conn,
    int64_t stream_id,
    size_t consumed,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_begin_headers)(
    nghttp3_conn *conn,
    int64_t stream_id,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_recv_header)(
    nghttp3_conn *conn,
    int64_t stream_id,
    int32_t token,
    nghttp3_rcbuf *name,
    nghttp3_rcbuf *value,
    uint8_t flags,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_end_headers)(
    nghttp3_conn *conn,
    int64_t stream_id,
    int fin,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_end_stream)(
    nghttp3_conn *conn,
    int64_t stream_id,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_stop_sending)(
    nghttp3_conn *conn,
    int64_t stream_id,
    uint64_t app_error_code,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_reset_stream)(
    nghttp3_conn *conn,
    int64_t stream_id,
    uint64_t app_error_code,
    void *user_data,
    void *stream_user_data
);

typedef int (*nghttp3_shutdown)(
    nghttp3_conn *conn,
    int64_t id,
    void *user_data
);

/*
 * Callbacks Structure
 */
struct nghttp3_callbacks {
    nghttp3_acked_stream_data acked_stream_data;
    nghttp3_stream_close stream_close;
    nghttp3_recv_data recv_data;
    nghttp3_deferred_consume deferred_consume;
    nghttp3_begin_headers begin_headers;
    nghttp3_recv_header recv_header;
    nghttp3_end_headers end_headers;
    nghttp3_end_stream end_stream;
    nghttp3_stop_sending stop_sending;
    nghttp3_reset_stream reset_stream;
    nghttp3_shutdown shutdown;
    nghttp3_begin_headers begin_trailers;
    nghttp3_recv_header recv_trailer;
    nghttp3_end_headers end_trailers;
};

/*
 * Memory Allocator
 */
struct nghttp3_mem {
    void *user_data;
    void *(*malloc)(size_t size, void *user_data);
    void (*free)(void *ptr, void *user_data);
    void *(*calloc)(size_t nmemb, size_t size, void *user_data);
    void *(*realloc)(void *ptr, size_t size, void *user_data);
};

/*
 * ============================================================================
 * API Functions
 * ============================================================================
 */

/*
 * Version Functions
 */
const nghttp3_info *nghttp3_version(int least_version);
int nghttp3_err_is_fatal(int error_code);
const char *nghttp3_strerror(int error_code);

/*
 * Settings Functions
 */
void nghttp3_settings_default(nghttp3_settings *settings);

/*
 * Memory Functions
 */
const nghttp3_mem *nghttp3_mem_default(void);

/*
 * Callback Management
 */
int nghttp3_callbacks_new(nghttp3_callbacks **pcallbacks);
void nghttp3_callbacks_del(nghttp3_callbacks *callbacks);
void nghttp3_callbacks_set_acked_stream_data(nghttp3_callbacks *callbacks, nghttp3_acked_stream_data cb);
void nghttp3_callbacks_set_stream_close(nghttp3_callbacks *callbacks, nghttp3_stream_close cb);
void nghttp3_callbacks_set_recv_data(nghttp3_callbacks *callbacks, nghttp3_recv_data cb);
void nghttp3_callbacks_set_deferred_consume(nghttp3_callbacks *callbacks, nghttp3_deferred_consume cb);
void nghttp3_callbacks_set_begin_headers(nghttp3_callbacks *callbacks, nghttp3_begin_headers cb);
void nghttp3_callbacks_set_recv_header(nghttp3_callbacks *callbacks, nghttp3_recv_header cb);
void nghttp3_callbacks_set_end_headers(nghttp3_callbacks *callbacks, nghttp3_end_headers cb);
void nghttp3_callbacks_set_end_stream(nghttp3_callbacks *callbacks, nghttp3_end_stream cb);
void nghttp3_callbacks_set_stop_sending(nghttp3_callbacks *callbacks, nghttp3_stop_sending cb);
void nghttp3_callbacks_set_reset_stream(nghttp3_callbacks *callbacks, nghttp3_reset_stream cb);
void nghttp3_callbacks_set_shutdown(nghttp3_callbacks *callbacks, nghttp3_shutdown cb);

/*
 * Connection Management
 */
int nghttp3_conn_client_new(
    nghttp3_conn **pconn,
    const nghttp3_callbacks *callbacks,
    const nghttp3_settings *settings,
    const nghttp3_mem *mem,
    void *user_data
);

int nghttp3_conn_server_new(
    nghttp3_conn **pconn,
    const nghttp3_callbacks *callbacks,
    const nghttp3_settings *settings,
    const nghttp3_mem *mem,
    void *user_data
);

void nghttp3_conn_del(nghttp3_conn *conn);

int nghttp3_conn_bind_control_stream(nghttp3_conn *conn, int64_t stream_id);

int nghttp3_conn_bind_qpack_streams(
    nghttp3_conn *conn,
    int64_t qenc_stream_id,
    int64_t qdec_stream_id
);

/*
 * Stream I/O
 */
ssize_t nghttp3_conn_read_stream(
    nghttp3_conn *conn,
    int64_t stream_id,
    const uint8_t *data,
    size_t datalen,
    int fin
);

ssize_t nghttp3_conn_writev_stream(
    nghttp3_conn *conn,
    int64_t *pstream_id,
    int *pfin,
    nghttp3_vec *vec,
    size_t veccnt
);

int nghttp3_conn_add_write_offset(
    nghttp3_conn *conn,
    int64_t stream_id,
    size_t n
);

/*
 * Request/Response
 */
int nghttp3_conn_submit_request(
    nghttp3_conn *conn,
    int64_t stream_id,
    const nghttp3_nv *nva,
    size_t nvlen,
    const nghttp3_data_reader *dr,
    void *stream_user_data
);

int nghttp3_conn_submit_response(
    nghttp3_conn *conn,
    int64_t stream_id,
    const nghttp3_nv *nva,
    size_t nvlen,
    const nghttp3_data_reader *dr
);

int nghttp3_conn_submit_trailers(
    nghttp3_conn *conn,
    int64_t stream_id,
    const nghttp3_nv *nva,
    size_t nvlen
);

int nghttp3_conn_submit_data(
    nghttp3_conn *conn,
    int64_t stream_id,
    const nghttp3_data_reader *dr
);

/*
 * Stream Control
 */
int nghttp3_conn_shutdown(nghttp3_conn *conn);
int nghttp3_conn_close_stream(nghttp3_conn *conn, int64_t stream_id, uint64_t app_error_code);
int nghttp3_conn_block_stream(nghttp3_conn *conn, int64_t stream_id);
int nghttp3_conn_unblock_stream(nghttp3_conn *conn, int64_t stream_id);
int nghttp3_conn_resume_stream(nghttp3_conn *conn, int64_t stream_id);

/*
 * Stream User Data
 */
int nghttp3_conn_set_stream_user_data(nghttp3_conn *conn, int64_t stream_id, void *user_data);
void *nghttp3_conn_get_stream_user_data(nghttp3_conn *conn, int64_t stream_id);

/*
 * Priority
 */
void nghttp3_pri_default(nghttp3_pri *pri);
int nghttp3_conn_set_stream_priority(nghttp3_conn *conn, int64_t stream_id, const nghttp3_pri *pri);
int nghttp3_conn_get_stream_priority(nghttp3_conn *conn, nghttp3_pri *pri, int64_t stream_id);

/*
 * Connection State
 */
int nghttp3_conn_is_client(const nghttp3_conn *conn);
int nghttp3_conn_is_stream_scheduled(const nghttp3_conn *conn, int64_t stream_id);

/*
 * QPACK Streams
 */
int nghttp3_conn_get_qpack_encoder_stream_id(const nghttp3_conn *conn, int64_t *pstream_id);
int nghttp3_conn_get_qpack_decoder_stream_id(const nghttp3_conn *conn, int64_t *pstream_id);

/*
 * Server Push
 */
int nghttp3_conn_submit_max_push_id(nghttp3_conn *conn);
int nghttp3_conn_cancel_push(nghttp3_conn *conn, uint64_t push_id);

/*
 * Utility Functions
 */
int nghttp3_client_stream_bidi(int64_t stream_id);
int nghttp3_server_stream_bidi(int64_t stream_id);
int nghttp3_client_stream_uni(int64_t stream_id);
int nghttp3_server_stream_uni(int64_t stream_id);

/*
 * NV Helper
 */
nghttp3_nv nghttp3_nv_new(
    const uint8_t *name,
    size_t namelen,
    const uint8_t *value,
    size_t valuelen,
    uint8_t flags
);

/*
 * Vec Functions
 */
nghttp3_vec nghttp3_vec_new(void);
size_t nghttp3_vec_len(const nghttp3_vec *vec);

/*
 * RCBuf Functions
 */
const uint8_t *nghttp3_rcbuf_get_buf(const nghttp3_rcbuf *rcbuf);
size_t nghttp3_rcbuf_get_len(const nghttp3_rcbuf *rcbuf);

/*
 * Data Reader
 */
nghttp3_data_reader nghttp3_data_reader_new(nghttp3_read_data_callback read_data);

/*
 * ============================================================================
 * NexaOS Extensions
 * ============================================================================
 */

/*
 * Stream Response Data (nh3 extension)
 */
typedef struct {
    uint8_t *name;
    size_t name_len;
    uint8_t *value;
    size_t value_len;
} nghttp3_header_field;

typedef struct {
    nghttp3_header_field *headers;
    size_t headers_len;
    uint8_t *body;
    size_t body_len;
    uint16_t status_code;
} nghttp3_stream_response_data;

nghttp3_stream_response_data *nghttp3_conn_get_stream_response_data(
    nghttp3_conn *conn,
    int64_t stream_id
);

void nghttp3_stream_response_data_free(nghttp3_stream_response_data *data);

/*
 * High-Level Client (nh3 extension)
 */
typedef struct nghttp3_client nghttp3_client;

int nghttp3_client_new(nghttp3_client **pclient);
void nghttp3_client_del(nghttp3_client *client);
int nghttp3_is_available(void);

#ifdef __cplusplus
}
#endif

#endif /* NGHTTP3_H */
