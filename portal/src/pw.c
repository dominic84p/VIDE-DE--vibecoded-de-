#include <pipewire/pipewire.h>
#include <spa/param/video/format-utils.h>
#include <spa/param/buffers.h>
#include <spa/param/props.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct {
    struct pw_thread_loop *loop;
    struct pw_stream *stream;
    struct spa_hook listener;
    uint32_t width;
    uint32_t height;
    uint32_t fps;
    uint32_t node_id;
    pthread_mutex_t mu;
    uint8_t *frame;
    size_t frame_bytes;
    int streaming;
} vide_pw_t;

static void on_process(void *userdata) {
    vide_pw_t *ctx = (vide_pw_t *)userdata;
    struct pw_buffer *b = pw_stream_dequeue_buffer(ctx->stream);
    if (!b) return;

    struct spa_buffer *buf = b->buffer;
    uint8_t *dst = buf->datas[0].data;
    if (!dst) {
        pw_stream_queue_buffer(ctx->stream, b);
        return;
    }

    uint32_t stride = ctx->width * 4;
    uint32_t size = stride * ctx->height;
    if (buf->datas[0].maxsize < size) {
        size = buf->datas[0].maxsize;
    }

    pthread_mutex_lock(&ctx->mu);
    if (ctx->frame && ctx->frame_bytes >= size) {
        memcpy(dst, ctx->frame, size);
    } else {
        memset(dst, 0, size);
    }
    pthread_mutex_unlock(&ctx->mu);

    buf->datas[0].chunk->offset = 0;
    buf->datas[0].chunk->stride = (int32_t)stride;
    buf->datas[0].chunk->size = size;
    buf->datas[0].chunk->flags = SPA_CHUNK_FLAG_NONE;

    pw_stream_queue_buffer(ctx->stream, b);
}

static void on_param_changed(void *userdata, uint32_t id, const struct spa_pod *param) {
    vide_pw_t *ctx = (vide_pw_t *)userdata;
    if (!param || id != SPA_PARAM_Format) return;

    uint8_t pod_buf[512];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod_buf, sizeof(pod_buf));
    const struct spa_pod *params[1];
    uint32_t stride = ctx->width * 4;
    uint32_t size = stride * ctx->height;

    params[0] = spa_pod_builder_add_object(
        &b,
        SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_buffers, SPA_POD_CHOICE_RANGE_Int(4, 2, 8),
        SPA_PARAM_BUFFERS_blocks, SPA_POD_Int(1),
        SPA_PARAM_BUFFERS_size, SPA_POD_Int((int32_t)size),
        SPA_PARAM_BUFFERS_stride, SPA_POD_Int((int32_t)stride),
        SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int((1 << SPA_DATA_MemFd))
    );
    pw_stream_update_params(ctx->stream, params, 1);
}

static void on_state_changed(void *userdata, enum pw_stream_state old, enum pw_stream_state state, const char *error) {
    (void)old;
    (void)error;
    vide_pw_t *ctx = (vide_pw_t *)userdata;
    if (state == PW_STREAM_STATE_STREAMING) {
        ctx->streaming = 1;
        ctx->node_id = pw_stream_get_node_id(ctx->stream);
    } else if (state == PW_STREAM_STATE_PAUSED) {
        ctx->node_id = pw_stream_get_node_id(ctx->stream);
        ctx->streaming = 0;
    } else if (state == PW_STREAM_STATE_ERROR || state == PW_STREAM_STATE_UNCONNECTED) {
        ctx->streaming = 0;
    }
}

static const struct pw_stream_events stream_events = {
    PW_VERSION_STREAM_EVENTS,
    .state_changed = on_state_changed,
    .param_changed = on_param_changed,
    .process = on_process,
};

void *pw_cast_start(uint32_t width, uint32_t height, uint32_t fps) {
    pw_init(NULL, NULL);
    vide_pw_t *ctx = (vide_pw_t *)calloc(1, sizeof(vide_pw_t));
    if (!ctx) return NULL;

    ctx->width = width;
    ctx->height = height;
    ctx->fps = fps ? fps : 30;
    ctx->frame_bytes = (size_t)width * height * 4;
    ctx->frame = (uint8_t *)calloc(1, ctx->frame_bytes);
    pthread_mutex_init(&ctx->mu, NULL);

    ctx->loop = pw_thread_loop_new("vide-screencast", NULL);
    if (!ctx->loop) {
        free(ctx->frame);
        free(ctx);
        return NULL;
    }

    pw_thread_loop_lock(ctx->loop);
    struct pw_properties *props = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Video",
        PW_KEY_MEDIA_CATEGORY, "Capture",
        PW_KEY_MEDIA_ROLE, "Screen",
        NULL
    );
    ctx->stream = pw_stream_new_simple(
        pw_thread_loop_get_loop(ctx->loop),
        "VIDE-DE ScreenCast",
        props,
        &stream_events,
        ctx
    );

    uint8_t pod_buf[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod_buf, sizeof(pod_buf));
    const struct spa_pod *params[1];
    struct spa_rectangle def_size = SPA_RECTANGLE(width, height);
    struct spa_fraction def_fps = SPA_FRACTION(ctx->fps, 1);

    params[0] = spa_pod_builder_add_object(
        &b,
        SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
        SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_Id(SPA_VIDEO_FORMAT_BGRx),
        SPA_FORMAT_VIDEO_size, SPA_POD_Rectangle(&def_size),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_Fraction(&def_fps)
    );

    pw_stream_connect(
        ctx->stream,
        PW_DIRECTION_OUTPUT,
        PW_ID_ANY,
        PW_STREAM_FLAG_DRIVER | PW_STREAM_FLAG_MAP_BUFFERS,
        params,
        1
    );

    pw_thread_loop_unlock(ctx->loop);
    pw_thread_loop_start(ctx->loop);
    return ctx;
}

uint32_t pw_cast_node_id(void *p) {
    vide_pw_t *ctx = (vide_pw_t *)p;
    if (!ctx) return 0;
    for (int i = 0; i < 60; i++) {
        pw_thread_loop_lock(ctx->loop);
        uint32_t id = pw_stream_get_node_id(ctx->stream);
        pw_thread_loop_unlock(ctx->loop);
        if (id != 0 && id != PW_ID_ANY) {
            return id;
        }
        usleep(25000);
    }
    return 0;
}

void pw_cast_push_rgba(void *p, const uint8_t *rgba, uint32_t w, uint32_t h) {
    vide_pw_t *ctx = (vide_pw_t *)p;
    if (!ctx || !rgba) return;
    size_t n = (size_t)w * h * 4;
    pthread_mutex_lock(&ctx->mu);
    if (n == ctx->frame_bytes && ctx->frame) {
        // Convert RGBA -> BGRx for PipeWire/OBS/Chromium compatibility.
        for (size_t i = 0; i + 3 < n; i += 4) {
            ctx->frame[i + 0] = rgba[i + 2];
            ctx->frame[i + 1] = rgba[i + 1];
            ctx->frame[i + 2] = rgba[i + 0];
            ctx->frame[i + 3] = 0xff;
        }
    }
    pthread_mutex_unlock(&ctx->mu);
    pw_thread_loop_lock(ctx->loop);
    if (ctx->streaming) {
        pw_stream_trigger_process(ctx->stream);
    }
    pw_thread_loop_unlock(ctx->loop);
}

void pw_cast_stop(void *p) {
    vide_pw_t *ctx = (vide_pw_t *)p;
    if (!ctx) return;
    pw_thread_loop_stop(ctx->loop);
    pw_stream_destroy(ctx->stream);
    pw_thread_loop_destroy(ctx->loop);
    pthread_mutex_destroy(&ctx->mu);
    free(ctx->frame);
    free(ctx);
}
