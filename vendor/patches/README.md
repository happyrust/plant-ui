# vendor/patches

`vendor/registry` 里**直接改过源码**的第三方 crate，每一处改动在这里留一份按**上游仓路径**写的 `git am` 补丁。
日常构建不读这里：`vendor/registry` 那份已经改好、`.cargo-checksum.json` 已同步。这里的文件是给「重新 vendor / 升级」用的：
先把补丁 `git am` 到对应的 fork 并推上去，把 `Cargo.toml` 里的 rev 指到新提交，再按 `.cargo/config.toml` 头上的说明重跑 `cargo vendor`，改动就不会丢。

cargo 不跟踪 vendor 源码的改动：改了 `vendor/registry/<crate>` 之后要 `cargo clean -p <crate>`（每个 target / profile 各一次）才会重编。

| 补丁 | 上游 | 基线 | 说明 |
|---|---|---|---|
| `0001-bevy_core_pipeline-only-queue-the-tonemapping-pipeline-for-HDR-views.patch` | https://gitee.com/happydpc/bevy | `9d8f7921` | 非 HDR 视图不排色调映射管线；否则缺 `EXT_color_buffer_float` 的 WebGL2 设备上一启动就 panic（ADR-0032 D7，`docs/evidence/2026-10-22-e3d-look.md` §5）。vendor 副本里的注释多一句「plant-ui local patch」，代码相同。 |
