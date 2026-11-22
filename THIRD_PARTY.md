# 第三方组件与模型

本仓库（字在 zzai / Eryu）自身代码以 [Apache License 2.0](LICENSE) 发布。下列第三方组件与模型
不在此许可范围内，各自遵循其上游许可；具体条款以上游仓库/发布页为准（此处不代为命名或许诺）。

## 语音模型

程序运行时使用的语音模型（ASR / VAD / 说话人识别 / 标点 / 降噪）均从
[k2-fsa/sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) 的 GitHub Releases 下载，
模型由各自的上游项目训练与发布，遵循上游许可。下载地址见
[`core/src/models.rs`](core/src/models.rs) 与 [`scripts/fetch-models.sh`](scripts/fetch-models.sh)，例如：

| 用途 | 来源 |
|---|---|
| 语音识别（SenseVoice 等） | https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models |
| 语音端点检测（silero_vad.onnx） | https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models |
| 标点（CT-Transformer 等） | https://github.com/k2-fsa/sherpa-onnx/releases/tag/punctuation-models |
| 说话人识别（3D-Speaker） | https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models |
| 降噪（GTCRN） | https://github.com/k2-fsa/sherpa-onnx/releases/tag/speech-enhancement-models |

`shell-win/assets/` 下的 `silero_vad.onnx`、`gtcrn_simple.onnx` 是上述发布包的本地副本，
仅为降低首次构建/运行的联网依赖；使用时同样遵循上游许可。

## Rust / Swift 依赖

Rust 依赖见 [`core/Cargo.lock`](core/Cargo.lock)、[`shell-win/Cargo.lock`](shell-win/Cargo.lock)；
macOS 壳使用 SwiftPM，见 [`app/Package.swift`](app/Package.swift)。各依赖遵循其 crate/包的
上游许可，完整许可见对应包仓库。

## 说明

- 本项目不重新分发上述模型的许可文本副本；如需再分发，请自行核对上游许可要求。
- 本文件只做来源指认，不对上游许可名称或兼容性作判断。
