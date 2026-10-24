//! 虚拟孔洞表的文件选择（ADR-0031 阶段一「导入行为」1、2 两条路）。
//!
//! 原生端：系统文件对话框。对话框是阻塞的，只能在别的线程上等，结果经
//! 一次性通道回来——与设置里「网格目录」那一套同款（`App::browse_mesh_dir`）。
//! 浏览器端：没有对话框，用一个藏起来的 `<input type=file>`；change 事件里
//! 把文件读成字节落进一个槽，宿主每帧轮询取走。两路最后都交出
//! `PickedHoleFile`，宿主喂给同一个导入器。

/// 一次选择拿到的孔洞表：原生端是路径（宿主再读盘），浏览器端是内存字节。
#[derive(Debug)]
pub enum PickedHoleFile {
    /// 原生端：对话框选中的路径。
    Path(std::path::PathBuf),
    /// 浏览器端：隐藏 input 读出的 `(文件名, 字节)`。文件名只用来取后缀。
    // 原生端构造不了这个变体（对话框只给路径），dead_code 只在非 wasm 构建冒头。
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Bytes { name: String, bytes: Vec<u8> },
}

/// 一次轮询的结果。
#[derive(Debug)]
pub enum HolePickPoll {
    /// 拿到文件。
    Picked(PickedHoleFile),
    /// 还在等（对话框弹着 / 文件读着）——宿主该安排下一帧再问。
    Waiting,
    /// 这趟结束、没拿到文件（取消 / 线程断了）——宿主撤掉选择器。
    // 浏览器端没有「取消」事件，这个变体只在原生端构造。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Finished,
}

/// 文件选择器的运行态（原生端）。对话框开在独立线程上。
#[cfg(not(target_arch = "wasm32"))]
pub struct HoleFilePick {
    rx: std::sync::mpsc::Receiver<Option<std::path::PathBuf>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl HoleFilePick {
    /// 开系统文件对话框。
    pub fn open() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title("选择虚拟孔洞表")
                .add_filter("虚拟孔洞表", &["xlsx", "csv"])
                .pick_file();
            let _ = tx.send(picked);
        });
        Self { rx }
    }

    pub fn poll(&mut self, ctx: &egui::Context) -> HolePickPoll {
        match self.rx.try_recv() {
            Ok(Some(path)) => HolePickPoll::Picked(PickedHoleFile::Path(path)),
            Ok(None) => HolePickPoll::Finished,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                HolePickPoll::Waiting
            }
            // 线程没了却没送回东西，只能当这次没选。
            Err(std::sync::mpsc::TryRecvError::Disconnected) => HolePickPoll::Finished,
        }
    }
}

/// `HtmlInputElement` / `Rc` / `Closure` 都不满足 `Send`，但 `App` 要能进 Bevy
/// system（`Local<Option<App>>` 要求 `Send`）。wasm32-unknown-unknown 是单线程目标，
/// 这里 unsafe 放行是安全的（与 plant-csg web.rs 同款）。
#[cfg(target_arch = "wasm32")]
struct SendSync<T>(T);
/// SAFETY: wasm32 单线程，不存在跨线程共享访问。
#[cfg(target_arch = "wasm32")]
unsafe impl<T> Send for SendSync<T> {}
/// SAFETY: 同上。
#[cfg(target_arch = "wasm32")]
unsafe impl<T> Sync for SendSync<T> {}

/// 文件选择器的运行态（浏览器端）。input 藏在 body 里，点它等于点「选文件」。
#[cfg(target_arch = "wasm32")]
pub struct HoleFilePick {
    inner: SendSync<HoleFilePickInner>,
}

#[cfg(target_arch = "wasm32")]
struct HoleFilePickInner {
    input: web_sys::HtmlInputElement,
    /// change 回调里读完的文件落在这里，宿主每帧取走。读盘是异步的，
    /// 回调和轮询两头都要摸它，套一层 `Rc<RefCell<_>>`。
    pending: std::rc::Rc<std::cell::RefCell<Option<(String, Vec<u8>)>>>,
    /// 监听器的闭包必须活着事件才会来，留一格自己拿着。
    _on_change: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>,
}

#[cfg(target_arch = "wasm32")]
impl HoleFilePick {
    /// 造一个隐藏 input 并点开文件选择。浏览器要求 click 必须落在用户手势里，
    /// 这里就是那一下手势——别挪到别的帧去。
    pub fn open() -> Option<Self> {
        use wasm_bindgen::JsCast;
        let window = web_sys::window()?;
        let document = window.document()?;
        let input: web_sys::HtmlInputElement = document
            .create_element("input")
            .ok()?
            .dyn_into()
            .map_err(|_| ())
            .ok()?;
        input.set_type("file");
        input.set_accept(".xlsx,.csv");
        input.style().set_property("display", "none").ok()?;
        document.body()?.append_child(&input).ok()?;
        let pending = std::rc::Rc::new(std::cell::RefCell::new(None));
        let on_change = {
            let pending = pending.clone();
            let input = input.clone();
            wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                // 同一个文件再选一次要再触发 change：读完把 value 清掉。
                let Some(file) = input.files().and_then(|files| files.get(0)) else {
                    return;
                };
                let name = file.name();
                let pending = pending.clone();
                // 闭包要保持 FnMut：`input` 只借用，克隆一份进 async 块去清 value。
                let input_reset = input.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                        Ok(buffer) => {
                            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                            *pending.borrow_mut() = Some((name, bytes));
                        }
                        Err(error) => {
                            web_sys::console::error_1(
                                &format!("读取虚拟孔洞表失败：{error:?}").into(),
                            );
                        }
                    }
                    input_reset.set_value("");
                });
            }) as Box<dyn FnMut(web_sys::Event)>)
        };
        input
            .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
            .ok()?;
        input.click();
        Some(Self {
            inner: SendSync(HoleFilePickInner {
                input,
                pending,
                _on_change: on_change,
            }),
        })
    }

    /// 取走 change 回调里读完的文件。没读完时催下一帧再来——
    /// `spawn_local` 的 future 落地不叫醒 egui，不催就睡死过去。
    pub fn poll(&mut self, ctx: &egui::Context) -> HolePickPoll {
        if let Some((name, bytes)) = self.inner.0.pending.borrow_mut().take() {
            return HolePickPoll::Picked(PickedHoleFile::Bytes { name, bytes });
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        HolePickPoll::Waiting
    }
}

/// 撤掉选择器时把 input 从 DOM 拎出来——别在 body 里留一个看不见的文件选择器。
#[cfg(target_arch = "wasm32")]
impl Drop for HoleFilePick {
    fn drop(&mut self) {
        if let Some(parent) = self.inner.0.input.parent_node() {
            let _ = parent.remove_child(&self.inner.0.input);
        }
    }
}
