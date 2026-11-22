// swift-tools-version:5.9
import PackageDescription
import Foundation

// 核心静态库由 scripts/build.sh 先构建（core/target/release/liberyu_core.a）
let pkgDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let coreLibDir = pkgDir + "/../core/target/release"

let package = Package(
    name: "Eryu",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "Eryu", targets: ["EryuApp"])
    ],
    targets: [
        .target(
            name: "EryuC",
            path: "Sources/EryuC",
            linkerSettings: [
                .unsafeFlags(["-L", coreLibDir]),
                .linkedLibrary("eryu_core"),
                // sherpa-onnx（C++）静态库链入后必须补 libc++，
                // 完整清单见 `cargo rustc --release -- --print native-static-libs`
                .linkedLibrary("c++"),
                // Rust std / cpal 需要
                .linkedFramework("CoreFoundation"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("Accelerate"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .executableTarget(
            name: "EryuApp",
            dependencies: ["EryuC"],
            path: "Sources/EryuApp"
        ),
    ]
)
