#include "include/eryu_core.h"

/*
 * 静态库按需链接：Rust 静态库里的符号只有在被引用时才会进最终二进制。
 * Swift 侧靠 `Core.keepABISymbolsAlive()` 引用，但 -O 下可能被优化掉，
 * 所以这里用 C 的 function pointer 表 + `used` 属性做一次强引用，
 * 保证整个 ABI 都进产物（也是 scripts/smoke.sh 的符号检查依据）。
 *
 * 新增 ABI 函数时：头文件、这里、Swift keepABISymbolsAlive 三处一起加。
 */
__attribute__((used)) static const void *eryu_abi_refs[] = {
    (const void *)&eryu_abi_version,
    (const void *)&eryu_core_new,
    (const void *)&eryu_core_free,
    (const void *)&eryu_set_event_cb,
    (const void *)&eryu_clear_event_cb,
    (const void *)&eryu_hotkey_down,
    (const void *)&eryu_hotkey_up,
    (const void *)&eryu_cancel,
    (const void *)&eryu_submit_result,
    (const void *)&eryu_get_hotwords,
    (const void *)&eryu_set_hotwords,
    (const void *)&eryu_polish_test,
    (const void *)&eryu_models_pull,
    (const void *)&eryu_get_last_result,
    (const void *)&eryu_drop_last_result,
    (const void *)&eryu_string_free,
    (const void *)&eryu_get_config_toml,
    (const void *)&eryu_set_config_toml,
    (const void *)&eryu_get_config_json,
    (const void *)&eryu_set_config_json,
    (const void *)&eryu_get_stats,
    (const void *)&eryu_get_history,
    (const void *)&eryu_delete_history,
    (const void *)&eryu_clear_history,
    (const void *)&eryu_retry_failed,
    (const void *)&eryu_models_list,
    (const void *)&eryu_set_engine,
    (const void *)&eryu_list_devices,
    (const void *)&eryu_note_foreground_app,
    (const void *)&eryu_meter_start,
    (const void *)&eryu_meter_stop,
    (const void *)&eryu_diagnose,
    (const void *)&eryu_debug_submit_text,
    (const void *)&eryu_debug_record,
    (const void *)&eryu_debug_panic,
    (const void *)&eryu_polish_test,
    (const void *)&eryu_run_command,
    (const void *)&eryu_polish_models,
    (const void *)&eryu_set_test_mode,
    (const void *)&eryu_failed_segments,
    (const void *)&eryu_retry_segment,
    (const void *)&eryu_decode_wav,
    (const void *)&eryu_get_profiles,
    (const void *)&eryu_set_profiles,
    (const void *)&eryu_set_active_profile,
    (const void *)&eryu_voice_state,
    (const void *)&eryu_voice_enroll_wav,
    (const void *)&eryu_voice_clear,
    (const void *)&eryu_voice_enroll_mic,
};

/* 让链接器看到一个真实使用点，避免整表被当成未用数据丢弃。 */
const void *const *eryu_abi_refs_ptr(void) { return eryu_abi_refs; }
