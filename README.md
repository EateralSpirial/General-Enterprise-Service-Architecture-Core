# gesac

**General Enterprise Service Architecture Core**（GESAC）提供 Rust 服务主机与设备节点，用于多主机账号接入、服务方案跟踪、签名软件分发、Redis 在线协调、离线更新与恢复。PAUH Daemon 及三种操作系统 adapter 是其中一个受管理的软件对象。

仓库：[EateralSpirial/General-Enterprise-Service-Architecture-Core](https://github.com/EateralSpirial/General-Enterprise-Service-Architecture-Core)。源码采用 [Apache License 2.0](LICENSE)。

当前运行契约与模块边界见 [runtime.md](docs/runtime.md)，执行验证见 [IMPLEMENTATION.md](docs/IMPLEMENTATION.md)，文件及部署状态的保留规则见 [ARTIFACT_INVENTORY.md](ARTIFACT_INVENTORY.md)。GESA 的整体企业架构继续由 [架构文档](docs/README.md) 描述。

## 服务主机与多主机节点

`gesac server init --root <private-server-root> --redis-config <private-redis-config>` 从标准输入读取账号凭据 JSON；账号密码分别用 scrypt 保存摘要。可选 `otp_seed` 启用 TOTP，登录时通过当前一次性验证码。Redis 配置包含 `url`，支持本机 Unix socket。非回环监听同时配置 `--tls-cert` 和 `--tls-key`。

本机管理命令包括 `server account-add`、`account-disable`、`plan`、`grant`、`publish`、`publish-self` 和 `status`。`server serve --root <root>` 运行服务；配置独立自更新 Home 后，`server supervise --root <root>` 会在已提交的核心版本切换后交接服务子进程。账号、会话、方案、授权、发布和更新回执属于独立 `server.sqlite`。

节点通过 `GESAC_HOME` 选择自己的私有目录：

```text
gesac node init
gesac node profile-add --file <host-profile.json>
gesac node subscribe --profile <host-id> --plan <plan-id>
gesac node register --file <local-instance.json>
gesac node check --instance <instance-id>
gesac node stage --instance <instance-id>
gesac node prepare --operation <operation-id>
gesac node apply --operation <operation-id>
gesac node verify --instance <instance-id> --version <version>
gesac node reconcile --operation <operation-id>
```

每个主机 profile 包含独立 `id`、服务端 `host_id` 和 `connection`。connection 使用下文的中心配置字段，并可通过 `ca_certificate` 指定该主机的私有 CA 文件。`profile-add` 从标准输入读取密码与口令。Linux 使用 Secret Service，Windows 使用 DPAPI，macOS 使用原生 Keychain；无可用系统密钥环的后台节点可显式选择 `sealed`：AES-256-GCM 加密凭据，包装密钥由本机私有目录和 ACL 保护；当前操作系统用户属于信任边界。Unix 明文文件模式仅用于隔离测试。

安装实例包含 `id`、`profile`、`plan`、`product`、`target`、`channel`，以及可选的本地 `adapter` 和 `auto_update`。adapter 是本地绝对可执行路径、参数数组和超时。发布物提供文件与签名，执行权限由本机注册提供。`node sync` 执行一次自动更新；`node watch` 周期跟踪启用了 `auto_update` 的对象。PAUH 由原有 Core 维护时钟触发，保持唯一调度者。

`node export` 导出原始签名和文件摘要；`node stage --offline <bundle>` 从离线包暂存，后续 prepare/apply 无需服务端或 Redis。`node self-source --profile <id>` 选择 GESAC 自身更新来源；下文独立启动器负责核心的自更新与失败恢复。服务主机通过 `publish-self` 发布与其签名密钥绑定的自更新制品。

`gesac uninstall` 使用独立清理进程移除该节点的配置、凭据、数据库、缓存与安装内容。正在执行或结果未决的更新先完成核对。由 GESAC 管理的软件、用户数据以及独立服务主机的业务数据库保留各自的生命周期。

安装器如需注册 systemd、Windows 定时任务或 launchd 服务，应同时写入私有 `lifecycle.json`，其中 `uninstall` 为本地 adapter 命令。卸载先调用该命令，并要求返回 `{"status":"removed"}`，然后删除凭据与安装目录。命令失败时保留配置供核对。Windows 上这些本地配置文件必须由当前用户拥有，写入权限限于该用户、SYSTEM 与 Administrators；管理员组作为文件所有者不满足此约束。PAUH 的安装器脚本提供三个平台上按精确注册身份清理的实现。

当前实现、三系统 CI、实际部署和剩余能力边界见 [实现与验收记录](docs/IMPLEMENTATION.md)。服务端可为账号配置 TOTP；未启用 TOTP 的无人值守节点仍使用密码和口令，不构成独立多因素认证。

## 安装并连接你的中心

安装 Rust stable（最低 1.88），获取源码后编译两个可执行文件：

```bash
git clone https://github.com/EateralSpirial/General-Enterprise-Service-Architecture-Core.git
cd General-Enterprise-Service-Architecture-Core
cargo build --locked --release
./target/release/gesac init
./target/release/gesac install
```

Windows 在 PowerShell 中依次运行 `cargo build --locked --release`、`.\target\release\gesac.exe init`、`.\target\release\gesac.exe install`。发行目录中须同时保留 `gesac` 和 `gesac-launcher` 两个可执行文件。

`init` 交互询问中心 HTTPS 网址、账号、可信发布公钥，以及隐藏输入的密码和口令。公钥应由可信渠道提供。配置位置：

| 平台 | 默认目录 | 密码、口令、会话 |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/gesac` 或 `~/.config/gesac` | Secret Service 系统密钥环，需 `secret-tool` 和已解锁会话 |
| Windows | `%LOCALAPPDATA%\GESAC` | 当前用户 DPAPI 加密文件，配合目录 ACL 校验 |

此前 `gesa` 的配置、凭据与更新状态保持在独立目录。首次使用本项目执行 `gesac init`，为 `gesac` 建立自己的配置与凭据记录。

`GESAC_HOME` 可指定绝对目录，命令无需重复传入网址或凭据。中心和账号保存在该目录的 `config.json`；密码和口令没有 CLI 参数。Unix 无桌面测试环境可在初始化时显式选择 `file`，此模式以 0600 文件保存测试秘密，目录要求 0700。正式凭据使用系统密钥存储。

`install` 将独立启动器放到配置目录的 `bin/gesac`（Windows 为 `bin\gesac.exe`）。将该 `bin` 目录加入 PATH，之后统一使用：

```bash
gesac login
gesac status
gesac check
gesac update
gesac verify
gesac watch
gesac rollback
gesac recover
```

常用命令输出统一 JSON，`schema: 1`、`ok`、`command` 和 `result` 可供脚本检查。失败返回非零退出码；参数帮助遵循常规 CLI 文本格式。`gesac --version` 显示当前实际运行版本。

修改中心、账号、公钥后，使用 `gesac credentials` 重新隐藏输入凭据；`gesac logout` 删除本机会话。中心会话过期或返回 401 时会进行一次重新认证。`watch` 是前台轮询，默认间隔 60 秒，可用 `--once` 运行一轮。

上述历史 Python 开发中心使用账号、密码与静态口令。密码与静态口令属于同类知识凭据；Rust 服务主机另外支持可选 TOTP，是否启用由账号策略决定。

## 快速测试整个闭环

需要 Rust stable、Python 3.12+；Redis 平滑切流实验需要 `redis-server`。完整进程测试面向 Linux；CI 配置覆盖 Windows 的 Rust 构建、权限和 DPAPI 测试。当前命名版本的实际验证状态见 [测试记录](docs/developer/cli-prototype/validation.md)。

```bash
cargo build --locked
python3 -m pip install -r tools/requirements.txt
cargo test --locked
python3 -m unittest discover -s tools -p 'test_*.py' -v
python3 tools/e2e.py
```

`e2e.py` 自动建立临时中心和临时账号，编译 0.1.1／0.1.2 两个真实 CLI 版本，测试升级、回滚、异常签名、版本重放、契约门槛、故障隔离、恢复与旧进程存活。测试完成后清理临时凭据；测试证据均为合成夹具。请在专用 checkout 中运行，它会依次重新构建 debug 二进制。

Redis 测试会启动隔离的本机实例；找不到 `redis-server` 时明确显示跳过。可用环境变量 `REDIS_SERVER` 指定其位置。

## 本机中心实验

中心服务为开发夹具，绑定 `127.0.0.1`。在仓库以外选择私有目录：

```bash
python3 tools/demo_center.py init --root /absolute/private/gesac-center
python3 tools/demo_center.py serve --root /absolute/private/gesac-center
```

`init` 隐藏询问密码和口令，输出发布公钥。客户端 `gesac init` 填入 `http://127.0.0.1:8787`，显式启用回环 HTTP，使用刚建立的账号和公钥。远程中心强制 HTTPS，禁止重定向和跨来源下载。

发布一个已测试的二进制需要独立迭代证据文件，格式与发布命令见 [协议文档](docs/developer/cli-prototype/protocol.md)。最快的完整演练入口是上述 `e2e.py`。

Redis 平滑切流演练使用外部 JSON 配置，例如：

```json
{"redis_host":"127.0.0.1","redis_port":6379,"namespace":"gesac-demo","requests":100}
```

```bash
python3 tools/rollout_demo.py --config /absolute/private/rollout.json
```

该实验运行稳定 HTTP 网关和新旧实例，检查新实例就绪，通过 Redis 原子切换路由，保留旧实例直到请求排空。Redis 密码也从配置文件读取。

## 原型边界

- CLI 自更新采用独立启动器与不可变版本目录：新调用使用新版本，已运行的进程完成原任务。启动器自身由发行包安装更新，并保留可信 bootstrap 恢复入口。
- 服务切流是独立的真实 Redis／HTTP 实验。生产模块调度、PostgreSQL 数据迁移、数据库引擎自更新和跨主机高可用仍需后续实现。
- 自动迭代覆盖“证据 → 确定性门槛 → 签名发布 → 安装 → 校验／回滚”。证据由发布方声明并签名；代码生成和 CI 证据来源验证属于外部 M2／发布流水线。
- 发布安全使用单个固定 Ed25519 公钥、摘要、序列高水位和过期检查。完整 TUF 根密钥轮换、委托、阈值签名及第三方审计仍需补充。
- CLI 健康检查验证协议、版本、平台和进程退出状态。企业业务正确性需要各模块的专门健康检查。
- 开发中心的签名私钥保存在测试目录中；生产中心应分离身份服务、受限签名服务及 HTTPS 网关。

实现细节见 [原型协议](docs/developer/cli-prototype/protocol.md) 和 [测试记录](docs/developer/cli-prototype/validation.md)。
