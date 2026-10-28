# GESAC

GESAC 是企业软件部署工具，提供命令行客户端、云端服务和企业网页。当前版本为 **0.4.0**，企业入口为 [yanhan.chat/gesa](https://yanhan.chat/gesa/)。

## 功能

- 企业账号登录：网页和客户端共用账号、密码。
- 设备登记：客户端保存设备 ID，云端按企业管理设备。
- 软件部署：选择软件、申请权限，授权后通过命令安装。
- 软件分发：支持 ZIP、Windows MSI 和 EXE，发布方配置安装参数及用户或整机范围。
- 自动升级：Windows 用户启用后，每 15 分钟检查已安装软件；计划任务在该用户登录期间运行。
- 部署记录：网页查看版本、安装进度、结果和恢复提示。
- 发布校验：Ed25519 签名与 SHA-256 校验软件发布包，保留版本和操作记录。
- 公网连接：企业电脑通过 HTTPS 连接服务。

## 使用

从企业网页或 [Releases](https://github.com/EateralSpirial/General-Enterprise-Service-Architecture-Core/releases) 下载客户端，在 PowerShell 中运行：

```powershell
.\gesac.exe configure --host https://yanhan.chat/gesa/api/
.\gesac.exe login
.\gesac.exe software
.\gesac.exe install 软件ID
.\gesac.exe auto-update 软件ID enable
.\gesac.exe status
```

账号和软件权限由发布方提供；整机安装及对应升级任务使用管理员 PowerShell。

## 源码编译

安装 Rust 后，在仓库根目录运行：

```sh
cargo build --release --locked
```

仓库固定 Rust 1.93.0，产物位于 `target/release/gesac`，Windows 为 `gesac.exe`。`src/` 包含客户端和服务端，`web/` 包含企业网页。`gesac server --help` 和 `gesac admin --help` 提供服务启动、账号管理、软件发布和授权命令。

## 许可

[Apache-2.0](LICENSE)
