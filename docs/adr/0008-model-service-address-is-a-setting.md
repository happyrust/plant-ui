# 模型服务地址是一项设置，默认 8021

`model_update_api::base_url()` 原先写死回落到 `http://127.0.0.1:8020`，而 gen-model 实际
监听在 8021——运行日志是 `Web 服务已启动: http://0.0.0.0:8021/api/v1`。

对不上还不是最糟的。`gen-model/DbOption.toml` 的注释交代了让端口的原因：**8020 的回环侧被
plant-web-server 的 surreal 占着**。也就是说指向 8020 不会干净地连不上，而是打进那个
SurrealDB 实例、拿回一个看着像模像样的 HTTP 错误，再被
`model_update_api::request` 当成「模型服务返回了错误」报出来。排查成本很高。

`plant-ui/DbOption.toml` 里那条 `http_api_addr = "0.0.0.0:8020"` 更是纯误导：它是**服务端
监听地址**的形状，而 `base_url()` 根本不读本文件，只读环境变量。

## Considered Options

- **只把默认值改成 8021**：一行改动。但换机器、换端口仍然只能靠环境变量，联调时得改启动方式。
- **让 gen-model 换回 8020**：文档（spec §7、§9 评审决议）写的就是 8020。但 8020 已经被占，
  换不回去。
- **启动时探测 8021 / 8020，用 `/health` 认服务**：能自愈，`{ status, project, sync_live,
  version }` 这个形状 SurrealDB 绝不会返。代价是多一层隐式行为，出错时更难解释到底连的是谁。

## Consequences

- 优先级定为**设置项 > 环境变量 > 出厂默认**。出厂默认 `DEFAULT_MODEL_API_URL` 放在绘制层
  的 `settings.rs`，那里只是个常量，不读环境变量；环境变量由宿主 `plant-ui-app` 解析后经
  `settings::State::adopt` 顶进去。绘制层不认识环境，这条边界不破。
- S6 设置窗多了一节「服务」。保存时归一化：去掉尾斜杠（`{base}{path}` 的拼法会拼出
  `//api/v1`），清空则退回出厂默认，不允许存下一个连不上任何东西的空串。
- `plant-ui/DbOption.toml` 里那两条 `http_api_*` 删掉，换成一句指路的注释。
- gen-model 那侧还欠一件事：`http_api` feature 既不在 `default` 也不在 `console` 里，
  而 `docs/specs/web-service-api.md` §2 明写「默认不启用，`console` feature 可包含它」。
  这是实现漂离了文档，把它补进 `console` 即可，`default` 不动。
