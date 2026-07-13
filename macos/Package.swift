// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SkaldMac",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "SkaldApp", targets: ["SkaldApp"]),
        .executable(name: "skald-native", targets: ["SkaldNative"]),
    ],
    targets: [
        .executableTarget(name: "SkaldApp"),
        .executableTarget(name: "SkaldNative"),
    ]
)
