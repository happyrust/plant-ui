# AMS 虚拟孔洞导入与实时布尔预览开发计划

## 当前基线

仓库：`D:/work/plant-code/old/plant-ui`

已完成的基础代码：

- `crates/plant-ui/src/virtual_hole.rs`
  - `VirtualHole`
  - CIR/RECT 切削体规格
  - E3D 姿态矩阵
  - 世界坐标 AABB
  - AABB 相交判断
- `crates/plant-ui-app/src/virtual_hole.rs`
  - XLSX/CSV 导入
  - `Code/_key`、`SubsMeterial/SubsMaterial` 边界兼容
  - Position、Shape、SizeWidth、SizeHeight、SizeThrowWall、Ori 解析
- `plant-ui` 虚拟孔洞单元测试已通过。
- `rs-plant` 原生端 `cargo check` 已通过。

当前仍缺少：导入入口、孔洞列表 UI、Manifold 差集、预览网格替换、浏览器 Worker 和恢复原模型。

## 阶段一：导入会话与映射

### 目标

把解析结果接入工作台状态，不改变正式模型。

### 实现

在 `plant-ui-app/src/main.rs` 增加：

```rust
struct VirtualHoleSession {
    holes: Vec<VirtualHole>,
    mappings: HashMap<String, Vec<RefU64>>,
    statuses: HashMap<String, VirtualHoleStatus>,
    previews: HashMap<RefU64, PreviewMesh>,
}
```

增加命令：

- `ImportVirtualHoles`
- `MapVirtualHole`
- `PreviewVirtualHole`
- `ClearVirtualHolePreview`
- `FocusVirtualHole`

导入行为：

1. 原生端使用文件选择器。
2. 浏览器端使用文件上传对象。
3. 调用现有 `import_file`。
4. 通过 `RelyItemREF` 自动映射 AMS 元素。
5. 映射失败时允许从模型树或搜索结果手动选择。
6. 每条记录显示解析、映射、相交和布尔状态。

验收：

- holes.xlsx 的 3 条记录全部进入列表。
- CIR 记录生成圆柱参数。
- RECT 记录生成矩形参数。
- 错误记录不阻断其他记录。

## 阶段二：三维孔洞预览层

### 目标

独立显示孔洞切削体，并支持定位、选择和恢复。

### 实现文件

- `crates/plant-ui-view3d/src/virtual_hole.rs`
- `crates/plant-ui-view3d/src/lib.rs`
- `crates/plant-ui/src/vm.rs`
- `crates/plant-ui/src/workbench/*`

新增 View3d API：

```rust
set_virtual_holes(Vec<VirtualHole>)
set_virtual_hole_selection(Option<String>)
focus_virtual_hole(&str)
set_virtual_hole_results(Vec<PreviewMesh>)
clear_virtual_holes()
```

显示规则：

- CIR：半透明圆柱。
- RECT：半透明长方体。
- 选中时显示编号、尺寸和深度。
- 未匹配实体的孔洞仍独立显示。
- 清除预览后恢复所有原始网格。

预览层不得复用尺寸标注层作为长期实体层，只复用毫米坐标转换和标签投影逻辑。

## 阶段三：布尔后端

### 统一接口

```rust
trait BooleanPreviewBackend {
    fn mesh_to_solid(&self, vertices: &[f32], indices: &[u32]) -> Result<SolidHandle>;
    fn make_cutter(&self, cutter: CutterSpec) -> Result<SolidHandle>;
    fn difference(&self, target: SolidHandle, cutter: SolidHandle) -> Result<MeshData>;
}
```

### 桌面端

- 复用 gen-model 的 native `manifold-csg` 转换逻辑。
- mesh 转 Manifold。
- 对同一目标实体的多个孔洞合并 cutter 后执行一次 difference。
- Manifold 结果转回渲染网格。
- 结果只进入 `VirtualHoleSession.previews`，不写回数据库和模型服务。

### 浏览器端

- 使用 `manifold-rs-wasm` 的 JS/WASM 绑定。
- 新增 `web/public/virtual-hole/worker.js`。
- Worker 接收目标网格和 cutter 参数，返回三角网格和状态。
- 主线程只管理 UI 和渲染。
- 缓存同一目标实体的 Manifold 输入。
- 孔洞尺寸或位置变化时只重建受影响 cutter。

## 阶段四：相交筛选与结果应用

算法：

1. 只取 `RelyItemREF` 映射的实体。
2. 计算孔洞 OBB/AABB。
3. 用实体包围盒执行宽相筛选。
4. 对候选实体执行精确 Manifold difference。
5. 无相交返回 `NoIntersection`。
6. 布尔失败保留原网格并返回 `Failed`。
7. 不切除无关联但空间相交的管道和设备。
8. 多孔洞命中同一实体时合并 cutter，避免顺序差异。

结果状态：

- `Ready`
- `NoIntersection`
- `Changed`
- `Failed`
- `Calculating`

## 阶段五：测试与验收

### 数据测试

- 附件 3 条记录导入。
- CIR 直径 325、300。
- RECT 尺寸 300×400。
- 深度均为 800。
- Position 三轴解析。
- `Y is Y and Z is -Z` 姿态解析。
- 前导零、Code、ItemREF、来源行号保留。
- SizeHeight=0 的圆孔有效。

### 几何测试

- 完全穿透。
- 部分相交。
- 无相交。
- 多孔洞同实体。
- 关联实体缺失。
- 非流形网格或布尔失败回退。
- 清除预览后原始网格恢复。

### 应用测试

- 原生端文件导入。
- 浏览器端文件导入。
- 自动映射和手动映射。
- 孔洞定位和关联实体定位。
- 原始/布尔结果切换。
- 楼板或墙体 X-Ray。
- Worker 运行时相机和拾取保持响应。
- 桌面 native 与浏览器 WASM 对同一输入进行包围盒、体积和三角数量抽样比对。

### 性能门槛

- 单孔单实体预览目标小于 200ms。
- 批量计算异步执行。
- 超过 200ms 显示计算状态。
- UI 不因布尔运算阻塞。

## 约束

- 只支持 CIR 和 RECT。
- 只切除 RelyItemREF 映射实体。
- 预览结果只存在内存。
- Position 按孔洞中心解释。
- SizeWidth 为圆孔直径。
- SizeThrowWall 为 cutter 长度。
- 正式模型写回、导出和版本管理不在本期。


## 实施偏差（2026-10-14 实施时定案，均经用户确认）

1. **浏览器端**（阶段三「浏览器端」）：原定 manifold-rs-wasm 的 JS/WASM 绑定，改为官方 manifold-3d@3.5.4（npm）+ Web Worker。内核与原生端 C++ v3.5.2 一致，对拍有抓手；版本取舍与更新办法见 web/public/virtual-hole/VENDOR.md。
2. **桌面端**（阶段三「桌面端」）：manifold-csg 不做硬依赖，挂默认开启的可选 feature irtual-hole-csg（plant-ui-view3d → plant-ui-app 一路打开）。关掉时预览功能不可用（UnavailableBackend 兜底，行状态如实报失败），导入与列表仍可用。
3. **范围**：五个阶段一次做完，不做分期交付。
4. **夹具**（阶段五「数据测试」）：测试夹具在代码内生成（XLSX 用 zip 现场造），不签入附件文件。
5. **RelyItemREF 解析**（阶段一）：按 库号_参考号（如 24381_145018）解析参考号。

实现层面的调整（不改变用户可见行为）：阶段三「统一接口」原定的 SolidHandle 句柄跨不过线程与 Worker 边界，降为按作业提交 + 轮询（submit / poll / forget），见 crates/plant-csg/src/lib.rs 的接口文档。

验收记录：docs/evidence/2026-10-14-ams-virtual-hole.md。决策沉淀：ADR-0031（docs/adr/0031-virtual-hole-preview-is-memory-only.md）。

## 审核修正（2026-10-26）

1. **映射帧号只留会话那一份**：宿主曾另记 `hole_map_epoch`，换库 / 重连（`reset_for_reconnect`）时只加自己那份、会话 `reset()` 不加，之后每次导入两边都差 1，`Evt::VirtualHoleTargets` 全被会话当过期包丢掉，行永远「确认目标中」。现在请求带 `session.map_epoch()`，整清也进映射帧，有回归测试。
2. **多个未加载目标的重试**：`hole_retry_pending` 从单个改成集合，谁的网格先落地谁先重发预览；原来同一拍几条 `no_instances` 回执只留最后一个。
3. **失败行可重试**：`begin_preview` 取 `Ready | Failed`，不再只有「清除」一条出路。
4. **重映射 / 清场重装后切削体摆回去**：`Clear` 连层一起撒（「清除」按钮说好的），但这两种场合会话里的孔洞还在，改为清完再发一次 `SetLayer`（`restore_hole_layer`）；否则「定位孔洞」静默无效、要重导入才回来。
5. **Worker 里 manifold 对象释放**：`boolean.js` 的基元、摆好位的切削体、差集结果统一在 `finally` 里 `delete()`，`cacheSet` 淘汰 / 替换的实体也删；原来每次预览都往 WASM 堆里漏。
6. **姿态解析**：`Ori` 认 PDMS / E3D 罗盘写法（N / S / E / W / U / D，含带负号的），给的是哪两根轴都能补出第三根；原来只认 X/Y/Z 且只认 Y+Z。
7. **回执耗时**：`VirtualHoleReceipt.elapsed_ms` 按实例累加后端报的耗时，原来写死 0。
8. **等不到的目标要销账**：「目标没实例 → 显示 → 等网格」那条路上，范围查空 / 网格全没加载成功 / 没有可画的几何时，这个目标下的行落 Failed 并说原因（`hole_target_verdict` / `VirtualHoleSession::fail_target`）；原来只认 `mesh_loaded > 0`，其余情形行永远「已就绪」。结算点从渲染回执改到 `rebuild_tree`（范围回包也过那里）。
9. **「定位目标」等几何落地再取景**：目标还没加载时先显示、记 `hole_focus_pending`，画出来了再发 `Focus`；原来立刻发，视口手上还没有它的包围盒，相机不动。
10. **后端懒建**：`HolePreview.backend` 改为第一次有作业要提才建；原来 `setup` 就建，浏览器端页面一开就起 Worker 并下载 manifold 的 wasm，哪怕从没导过孔洞表。没建过的后端不为 `forget` 而建（没算过就没缓存过）。
