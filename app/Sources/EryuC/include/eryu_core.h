/*
 * eryu_core.h — Eryu 核心 C ABI v1（冻结契约）
 *
 * 规则（DESIGN §3.1）：
 *  - 所有函数首参是 opaque 的 EryuCore*；命令"入队即返回"，不阻塞调用线程。
 *  - 入参字符串只在调用期间有效；core→壳回调里的字符串同样只在回调期间有效，
 *    壳必须先复制再跨线程传递。
 *  - 查询类返回的字符串由 Rust 分配，只能用 eryu_string_free 释放。
 *  - 回调在 core 的单一派发线程上串行调用（有序），关机流程走「拒绝新命令 → 等回调结束 → 释放」。
 *  - panic 不会穿过 FFI：core 内部 catch_unwind，panic 记为 error 事件并停用该实例。
 */
#ifndef ERYU_CORE_H
#define ERYU_CORE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct EryuCore EryuCore;

/* 事件回调：json 为单行 UTF-8 JSON，仅在回调期间有效。 */
typedef void (*EryuEventCb)(const char *json, void *ctx);

/* 返回 (major << 16) | minor。壳必须在 eryu_core_new 之前比对 major。 */
uint32_t eryu_abi_version(void);

/* 创建核心。config_dir 为数据目录（UTF-8），可为 NULL 表示使用默认目录。
 * 失败返回 NULL。 */
EryuCore *eryu_core_new(const char *config_dir);

/* 释放核心：拒绝新命令 → 等待在途回调结束 → 停线程 → 释放。可传 NULL。 */
void eryu_core_free(EryuCore *core);

/* 订阅事件（每条新连接只应订阅一次）。返回 0 成功，-1 参数错误，-2 已关机。 */
int32_t eryu_set_event_cb(EryuCore *core, EryuEventCb cb, void *ctx);

/* 取消订阅（关机屏障的一部分）：之后不再有回调。 */
int32_t eryu_clear_event_cb(EryuCore *core);

/* 热键按下 / 松开（由壳的热键后端转译）。 */
int32_t eryu_hotkey_down(EryuCore *core);
int32_t eryu_hotkey_up(EryuCore *core);

/* 取消当前会话（Inserting 阶段会被拒绝，见 DESIGN §4.5）。 */
int32_t eryu_cancel(EryuCore *core);

/* 壳执行 submit_request 后回报：status = "ok" | "undelivered" | "unknown"。
 * 同一 task_id 只接受一次；重复回报返回 -3。 */
int32_t eryu_submit_result(EryuCore *core, const char *task_id,
                           const char *status, const char *detail);

/* ======================== ABI v1.2 新增（M2：热词表） ======================== */

/* 热词表（JSON）：{"rules":[{"from":"艾瑞","to":"Eryu"}],"path":"/Users/…/hotwords.toml"} */
char *eryu_get_hotwords(EryuCore *core);

/* 保存热词表（接受 {"rules":[…]} 或裸数组）。成功 0 并广播 hotwords_changed 事件；
 * 参数错误 -1；已关机 -2；格式/写入失败 -3（*err_out 收说明，用 eryu_string_free 释放）。 */
int32_t eryu_set_hotwords(EryuCore *core, const char *json, char **err_out);

/* ---- v1.3 ---------------------------------------------------------------- */

/* 用当前配置润色一句示例文本（设置页「测试连接」）。成功返回润色后的 UTF-8 串
 * （调用方负责 eryu_string_free），失败返回 NULL 并往 err_out 写中文原因。
 * 注意：这是同步网络调用，最坏阻塞 polish_timeout_ms，别在主线程调。 */
char *eryu_polish_test(EryuCore *core, char **err_out);

/* 取回最近一次结果（UTF-8，调用方用 eryu_string_free 释放；无结果返回 NULL）。 */
char *eryu_get_last_result(EryuCore *core);

/// 用户已经手动取回上次结果：清掉崩溃恢复用的暂存（不影响历史）。
/// v1.1 新增。返回 0 成功，-1 参数错误。
int32_t eryu_drop_last_result(EryuCore *core);

void eryu_string_free(char *s);

/* ======================== ABI v1.1 新增（M1b：设置页 / 统计 / 历史 / 失败重试） ======================== */

/* 当前生效配置（TOML 文本）。调用方用 eryu_string_free 释放；失败返回 NULL。 */
char *eryu_get_config_toml(EryuCore *core);

/* 校验并保存配置。成功返回 0；参数错误 -1；已关机 -2；校验/写入失败 -3，*err_out
 * 收到由 Rust 分配的错误说明（调用方用 eryu_string_free 释放）。
 * **失败时保留旧配置**（DESIGN §5.1 验收）。成功后 core 发 config_changed 事件。 */
int32_t eryu_set_config_toml(EryuCore *core, const char *toml, char **err_out);

/* 有效配置（JSON，设置页直接绑定）。字段见 config::Config::to_json。
 * 调用方用 eryu_string_free 释放；失败返回 NULL。 */
char *eryu_get_config_json(EryuCore *core);

/* 只更新传入的字段并保存（JSON）。返回 0 成功；-1 参数错误；-2 已关机；
 * -3 校验/写入失败（*err_out 收错误说明，用 eryu_string_free 释放）；失败保留旧配置。 */
int32_t eryu_set_config_json(EryuCore *core, const char *json, char **err_out);

/* 统计（JSON）：{"sessions":n,"segments":n,"chars":n,"speech_ms":n,"decode_ms":n,
 * "today":{"sessions":n,"chars":n},"days":[{date,sessions,chars,speech_ms}],"since":"YYYY-MM-DD"} */
char *eryu_get_stats(EryuCore *core);

/* 转写历史（JSON 数组，最新在前，最多 limit 条）：每项
 * {"id":"…","at":"2026-11-19T01:02:03+08:00","text":"…","status":"inserted|pending|failed",
 *  "app":"…","chars":n,"ms":n,"retryable":true|false}
 * 历史关闭时返回 []。 */
char *eryu_get_history(EryuCore *core, int32_t limit);

/* 删除单条历史（UI 负责三级确认文案）。返回 0 成功，-1 未找到。 */
int32_t eryu_delete_history(EryuCore *core, const char *id);

/* 清空历史。返回 0 成功。 */
int32_t eryu_clear_history(EryuCore *core);

/* 用暂存音频重跑上一次失败的识别（若暂存已被清理返回 -4）。 */
int32_t eryu_retry_failed(EryuCore *core);

/* 可用引擎/模型清单（JSON 数组，含 sense/qwen3/stub）：每项
 * {"id":"sense|qwen3","name":"…","installed":true|false,"active":true|false,
 *  "size_bytes":n,"note":"…"} */
char *eryu_models_list(EryuCore *core);

/* 切换引擎（"sense" | "qwen3" | "stub"）。下次录音生效；模型缺失返回 -4（附诊断）。
 * 成功后 core 发 config_changed 事件。 */
int32_t eryu_set_engine(EryuCore *core, const char *id);

/* 输入设备清单（JSON）：{"devices":[{"name":"…","default":true}],"selected":"…","default":"…"} */
char *eryu_list_devices(EryuCore *core);

/* 记下前台 app 名（历史条目归属用）；空串表示未知。 */
int32_t eryu_note_foreground_app(EryuCore *core, const char *name);

/* 麦克风电平测试（设置页「输入电平」用）：开始后按 ~15Hz 发 meter 事件
 * {"rms":0.0..1.0,"peak":0.0..1.0,"silent":true|false}；不含音频样本，不落盘。 */
int32_t eryu_meter_start(EryuCore *core);
int32_t eryu_meter_stop(EryuCore *core);

/* 环境体检文本（权限/模型/设备/配置），给设置页「诊断」用；M1c 接 `eryu diagnose` 子命令。 */
char *eryu_diagnose(EryuCore *core);

/* ---- 调试/冒烟接口（不进发布路径，但保留在 ABI 里便于自动化验证） ---- */
int32_t eryu_debug_submit_text(EryuCore *core, const char *text);
/* 不按热键也能验证采音→识别链路：开始录音，seconds 秒后自动停下并识别（1..10）。 */
int32_t eryu_debug_record(EryuCore *core, int32_t seconds);
int32_t eryu_debug_panic(EryuCore *core);

/* ---- v1.4 ------------------------------------------------------------ */
/* 选区指令改写：把 text 按 instruction 改写（同步网络调用，请在后台线程调用）。
   成功返回改写后的文字（用 eryu_string_free 释放），失败返回 NULL 并写 *err_out。 */
char *eryu_run_command(EryuCore *core, const char *instruction, const char *text, char **err_out);

/* ---------- v1.5 ---------- */

/* 下载并安装模型（后台线程，进度走 model_progress 事件）。
   0 = 已开始；-1 参数错误；-3 未知 id；-4 已有下载在跑；-5 已经安装。 */
int32_t eryu_models_pull(EryuCore *core, const char *id, char **err_out);

/* ---------- v1.6：润色 profile ---------- */

/* 返回 JSON：{"active":"日常","profiles":[{"name","icon","level","prompt","active"}],"path"} */
char *eryu_get_profiles(EryuCore *core);
/* 整体替换；返回 0 成功 / -1 参数 / -3 格式或写入失败（错误串写到 err_out） */
int32_t eryu_set_profiles(EryuCore *core, const char *json, char **err_out);
/* 切换当前档；名字不存在返回 -3 */
int32_t eryu_set_active_profile(EryuCore *core, const char *name, char **err_out);

/* ---------------------------------------------------------------- v1.9 */
/* 只录我的声音（声纹门控）：状态 JSON {enrolled,dim,created,model_ready,model_path,enabled,threshold} */
char *eryu_voice_state(EryuCore *core);
int32_t eryu_voice_enroll_mic(EryuCore *core, int32_t seconds, char **err_out);
/* 用一段 16k WAV 注册声纹；返回 0 成功 / -1 参数 / -3 失败（原因写 *err_out） */
int32_t eryu_voice_enroll_wav(EryuCore *core, const char *wav_path, char **err_out);
/* 删除已注册的声纹；返回 0 成功 / -1 参数 / -3 失败 */
int32_t eryu_voice_clear(EryuCore *core, char **err_out);

/* ---------------------------------------------------------------- v1.8 */
/* 自测用：把一段 16k 单声道 WAV 当成一次录音走完整链路（不需要麦克风）
   返回 0 已受理；-1 参数错误；-3 读不出来（原因写 *err_out） */
int32_t eryu_decode_wav(EryuCore *core, const char *wav_path, char **err_out);

/* ---------------------------------------------------------------- v1.7 */
/* 部分成功后的「只重试这一段」：序号来自 eryu_failed_segments 返回的数组 */
char *eryu_failed_segments(EryuCore *core);
/* 返回 0 已受理；-1 参数错误；-3 该段没有可重试的音频（文字原因写 *err_out，需 eryu_string_free） */
int32_t eryu_retry_segment(EryuCore *core, int32_t index, char **err_out);

/* ---- v1.11 ---- */

/* 自测模式：自动化路径（--decode-wav / --soak / --self-test）不写历史、不落盘。
   on = 0 关闭，其它开启。返回 0 = 成功，-1 = 空指针。 */
int32_t eryu_set_test_mode(EryuCore *core, int32_t on);

/* ---- v1.12 ---- */

/* 最近 40 条局域网请求摘要（JSON 数组，只有时间/来源/路径/状态/耗时，**不含内容**）。
   字符串由调用方 eryu_string_free。 */
char *eryu_lan_log(EryuCore *core);

/* ---- v1.13 ---- */
/* 问润色服务端有哪些模型（用已保存的 Key）。
 * 成功返回 JSON 数组字符串，调用方用 eryu_string_free 释放；失败返回 NULL 并写 err_out。
 */
char *eryu_polish_models(EryuCore *core, char **err_out);

/* ---- v1.14 ---- */
/* 上传诊断包（JSON 文本）到服务端。成功返回服务端响应体（截断 500 字），
   调用方用 eryu_string_free 释放；失败返回 NULL 并写 err_out（给用户看的中文原因）。
   0/-1/-3 分别对应「成功」「失败」「没设上传地址」。 */
int32_t eryu_upload_diag(EryuCore *core, const char *url, const char *body, char **detail_out);

#ifdef __cplusplus
}
#endif
#endif /* ERYU_CORE_H */
