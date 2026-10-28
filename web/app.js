const app = document.querySelector("#app");
const state = {
  session: null,
  devices: [],
  products: [],
  deployments: [],
  downloads: [],
  device: "",
  page: location.hash.slice(1) || "overview",
  error: "",
  recovery: "",
};
const pages = {
  overview: ["交付概览", "从一台电脑开始，让软件交付有迹可循。"],
  setup: ["接入电脑", "下载命令行工具，在公司电脑上完成首次登录。"],
  software: ["软件目录", "为当前电脑选择软件，授权后在本机安装。"],
  history: ["安装记录", "查看电脑回传的安装结果与更新信息。"],
};
const statuses = {
  reserved: "已创建安装任务",
  installing: "安装中",
  installed: "已安装",
  failed: "安装失败",
  restart_required: "安装完成，需重启",
};
const esc = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const time = (value) =>
  new Date(value * 1000).toLocaleString("zh-CN", { hour12: false });
const command = (text) =>
  /* HTML */ `<div class="command">
    <code>${esc(text)}</code
    ><button class="copy" data-copy="${esc(text)}" aria-label="复制命令">
      复制
    </button>
  </div>`;
const cli = (text) => `.\\gesac.exe ${text}`;
const productArg = (id) => `'${String(id).replaceAll("'", "''")}'`;
const badge = (text, type = "") =>
  /* HTML */ `<span class="badge ${type}">${esc(text)}</span>`;
const mark =
  '<span class="mark" aria-hidden="true"><i></i><i></i><i></i></span>';
const empty = (title, text, link = "") =>
  /* HTML */ `<div class="empty">
    <span class="empty-symbol" aria-hidden="true">＋</span>
    <h3>${title}</h3>
    <p>${text}</p>
    ${link}
  </div>`;
const link = (page, text) =>
  /* HTML */ `<a class="button secondary" href="#${page}"
    >${text} <span aria-hidden="true">↗</span></a
  >`;
const errorBox = () =>
  state.error
    ? /* HTML */ `<div class="error" role="alert">
        <strong>${esc(state.error)}</strong>
        <p>${esc(state.recovery)}</p>
        <button data-action="refresh" class="text-button">重新尝试</button>
        <details>
          <summary>错误详情</summary>
          <p>${esc(state.errorDetail)}</p>
        </details>
      </div>`
    : "";

async function api(
  path,
  options = {},
  binary = false,
) /* GATE: authenticated API authority */ {
  const response = await fetch(`./api/${path}`, {
    credentials: "same-origin",
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const data =
    binary && response.ok ? await response.blob() : await response.json();
  if (!response.ok) /* GATE: service rejection requires recovery */ {
    if (response.status === 401) state.session = null; // GATE: expired session requires login.
    throw Object.assign(new Error(data.error), {
      recovery: data.recovery,
      status: response.status,
    });
  }
  return data;
}

function login() /* GATE: display prerequisite and recovery for unavailable actions */ {
  document.querySelector("#notice").textContent = "";
  app.innerHTML = /* HTML */ `<div class="login-layout">
    <section class="login-story">
      <a class="brand" href="#overview"
        >${mark}<span>GESA<small>企业软件交付</small></span></a
      >
      <div class="story-body">
        <span class="eyebrow">SOFTWARE, DELIVERED.</span>
        <h1>软件到位。<br />工作就绪。</h1>
        <p>从授权到安装，让每一次交付<br />都有清晰的下一步。</p>
        <ol class="story-steps">
          <li><b>01</b> 接入公司电脑</li>
          <li><b>02</b> 选择并申请软件</li>
          <li><b>03</b> 安装与持续更新</li>
        </ol>
      </div>
      <div class="story-foot">GESA / ENTERPRISE DELIVERY <span>↗</span></div>
    </section>
    <main id="main" class="login-main" tabindex="-1">
      <div class="login-card">
        <span class="eyebrow">企业工作台</span>
        <h2>登录你的企业账号</h2>
        <p class="muted">使用发布方提供的账号与密码，开始接入。</p>
        ${errorBox()}
        <form id="login-form">
          <label for="account">企业账号</label
          ><input
            id="account"
            name="account"
            autocomplete="username"
            required
            placeholder="输入企业账号"
          /><label for="password">密码</label
          ><input
            id="password"
            name="password"
            type="password"
            autocomplete="current-password"
            required
            placeholder="输入密码"
          /><button class="button primary" type="submit">
            进入工作台 <span aria-hidden="true">→</span>
          </button>
        </form>
        <p class="login-help">尚未收到账号或忘记密码？请联系软件发布方。</p>
        <div class="login-note">
          <strong>一套账号，连接网站与电脑</strong>
          <p>
            在 Windows 11 电脑上使用 GESAC 命令行工具登录。通过公网 HTTPS
            连接，无需安装额外网络工具。
          </p>
        </div>
      </div>
      <footer>GESA · 企业软件交付平台 <span>Windows 11 / x86-64</span></footer>
    </main>
  </div>`; // GATE: required account/password fields enforce authenticated entry.
}

function shell() {
  document.querySelector("#notice").textContent = "";
  const [title, subtitle] = pages[state.page] || pages.overview;
  const selected = state.devices.find((d) => d.id === state.device);
  app.innerHTML = /* HTML */ `<div class="workspace">
    <aside class="sidebar">
      <a class="brand" href="#overview"
        >${mark}<span>GESA<small>企业软件交付</small></span></a
      >
      <div class="side-caption">工作台 / WORKSPACE</div>
      <nav aria-label="主要导航">
        ${Object.entries(pages)
          .map(
            ([id, [label]], index) =>
              /* HTML */ `<a
                href="#${id}"
                ${state.page === id ? 'aria-current="page"' : ""}
                ><span class="nav-number">0${index + 1}</span>${label}<span
                  class="nav-arrow"
                  >↗</span
                ></a
              >`,
          )
          .join("")}
      </nav>
      <div class="side-bottom">
        <div class="identity">
          <span class="avatar">${esc(state.session.label.slice(0, 1))}</span>
          <div>
            <strong>${esc(state.session.label)}</strong
            ><small>${esc(state.session.account)}</small>
          </div>
        </div>
        <button class="logout" data-action="logout">
          退出登录 <span>↗</span>
        </button>
        <p>企业交付 · 公网 HTTPS</p>
      </div>
    </aside>
    <div class="workspace-body">
      <header class="topbar">
        <span class="breadcrumb">企业工作台 <span>/</span> ${title}</span>
        <div class="device-control">
          <label for="device">当前电脑</label
          ><select id="device">
            <option value="">
              ${state.devices.length ? "选择电脑" : "尚未接入电脑"}
            </option>
            ${state.devices.map((d) => /* HTML */ `<option value="${esc(d.id)}" ${d.id === state.device ? "selected" : ""}>${esc(d.name)}</option>`).join("")}</select
          ><button class="refresh" data-action="refresh" aria-label="刷新数据">
            ↻ <span>刷新</span>
          </button>
        </div>
      </header>
      <main id="main" tabindex="-1">
        <div class="page-heading">
          <div>
            <span class="eyebrow">GESA / ${state.page.toUpperCase()}</span>
            <h1>${title}</h1>
            <p>${subtitle}</p>
          </div>
          <span class="edition">企业版<br /><b>01 / WINDOWS</b></span>
        </div>
        ${errorBox()}${({ overview, setup, software, history }[state.page] || overview)()}
        <footer class="content-footer">
          <span>GESA · 交付有据，更新有序</span
          ><span
            >${selected ? `最后联系 ${time(selected.last_seen)}` : "从接入第一台电脑开始"}</span
          >
        </footer>
      </main>
    </div>
  </div>`;
}

function overview() {
  const authorized = state.products.filter((p) => p.authorized).length;
  return /* HTML */ `<section class="overview-hero">
      <div>
        <span class="eyebrow">你的软件交付路径</span>
        <h2>
          ${state.devices.length ? "电脑已接入，继续下一步。" : "让第一台电脑，准备就绪。"}
        </h2>
        <p>
          ${state.devices.length ? "选择所需软件，由发布方授权，再在电脑上执行安装。" : "下载 GESAC，在 PowerShell 中登录企业账号，电脑就会出现在这里。"}
        </p>
        ${link(state.devices.length ? "software" : "setup", state.devices.length ? "前往软件目录" : "开始接入电脑")}
      </div>
      <div class="hero-art" aria-hidden="true">
        <span>G</span>
        <div class="art-label">CONNECT<br />SELECT<br />DELIVER</div>
      </div>
    </section>
    <section class="stats" aria-label="交付统计">
      <div>
        <span>已接入电脑</span
        ><strong>${state.devices.length}<small>台</small></strong>
      </div>
      <div>
        <span>当前电脑已获授权</span
        ><strong>${authorized}<small>款软件</small></strong>
      </div>
      <div>
        <span>当前电脑安装记录</span
        ><strong>${currentReceipts().length}<small>条</small></strong>
      </div>
    </section>
    <div class="section-title">
      <h2>三步完成首次交付</h2>
      <span>从网站到你的电脑</span>
    </div>
    <div class="journey">
      <article>
        <span class="step-number">01</span>
        <h3>接入电脑</h3>
        <p>
          下载 Windows 命令行工具，配置服务地址并登录。设备身份保留在当前
          Windows 用户下。
        </p>
        ${link("setup", "下载与接入")}
      </article>
      <article>
        <span class="step-number">02</span>
        <h3>选择软件</h3>
        <p>为电脑申请所需软件。发布方完成授权后，复制安装命令并在本机运行。</p>
        ${link("software", "浏览软件")}
      </article>
      <article>
        <span class="step-number">03</span>
        <h3>确认交付</h3>
        <p>查看安装回执。按需开启自动更新，让已安装的软件持续跟随新版本。</p>
        ${link("history", "查看安装记录")}
      </article>
    </div>
    <div class="note">
      <strong>以发布方提供的软件为准</strong>
      <p>
        目录仅展示实际发布的软件。验收示例用于验证交付流程，业务软件以发布方说明为准；目录为空时可先完成电脑接入。
      </p>
    </div>`;
}

function setup() {
  const download = state.downloads.find((d) => d.target === "windows-x86_64");
  return /* HTML */ `<div class="setup-grid">
      <section class="panel">
        <div class="section-title">
          <h2><span class="inline-step">01</span> 下载命令行工具</h2>
          ${badge("Windows 11 · x86-64")}
        </div>
        ${
          download
            ? /* HTML */ `<div class="download-title">
                  <span class="file-icon">&gt;_</span>
                  <div>
                    <h3>gesac.exe</h3>
                    <p>GESAC · Windows 命令行工具</p>
                  </div>
                </div>
                <button class="button primary" data-action="download">
                  下载 gesac.exe <span>↓</span>
                </button>
                <dl class="metadata">
                  <dt>版本</dt>
                  <dd>${esc(download.version)}</dd>
                  <dt>文件大小</dt>
                  <dd>${(download.size / 1048576).toFixed(2)} MB</dd>
                  <dt>SHA-256</dt>
                  <dd class="digest">${esc(download.sha256)}</dd>
                </dl>`
            : empty(
                "下载文件尚未就绪",
                "发布方尚未上传 Windows 命令行工具。请联系发布方，或稍后刷新查看。",
              )
        }
        <p class="hint">
          这是命令行程序。下载后在所在文件夹打开 PowerShell，执行右侧命令。
        </p>
      </section>
      <section class="panel">
        <h2><span class="inline-step">02</span> 配置并登录</h2>
        <p>在下载文件夹打开 PowerShell，逐条运行：</p>
        ${command(cli("configure --host https://yanhan.chat/gesa/api/"))}${command(cli("login"))}
        <p class="hint">
          登录时输入本网站的企业账号与密码。密码输入不会显示在终端中。登录成功后点击「刷新」，查看这台电脑。
        </p>
        <div class="subnote">
          首次接入面向 Windows 11
          x86-64。请保留工具的数据目录，以保留这台电脑的身份。
        </div>
      </section>
    </div>
    <section class="panel device-panel">
      <div class="section-title">
        <h2><span class="inline-step">03</span> 确认电脑已接入</h2>
        <button class="text-button" data-action="refresh">刷新设备 ↻</button>
      </div>
      ${
        state.devices.length
          ? /* HTML */ `<div class="device-list">
                ${state.devices
                  .map(
                    (d) =>
                      /* HTML */ `<article class="device-row">
                        <span class="computer-icon" aria-hidden="true">▣</span>
                        <div>
                          <h3>${esc(d.name)}</h3>
                          <p>
                            ${esc(d.os)} / ${esc(d.arch)} · 首次接入
                            ${time(d.first_seen)}
                          </p>
                          <code>${esc(d.id)}</code>
                        </div>
                        <div class="contact">
                          <small>最后联系</small
                          ><time>${time(d.last_seen)}</time>
                        </div>
                      </article>`,
                  )
                  .join("")}
              </div>
              <p class="hint">
                最后联系是工具最近一次访问服务的时间，不能据此判断电脑是否在线。运行
                ${esc(cli("sync"))} 后可刷新联系时间。
              </p>`
          : empty(
              "等待第一台电脑接入",
              "完成上方登录命令后刷新。网站登录不会自动登记电脑。",
            )
      }
    </section>`;
}

function software() /* GATE: display prerequisite and recovery for unavailable actions */ {
  if (!state.device)
    /* GATE: choose an enrolled computer first */
    return empty(
      "先接入并选择一台电脑",
      "软件权限和安装命令对应具体电脑。完成工具登录后，在页头选择电脑。",
      link("setup", "接入电脑"),
    ); // GATE: device-scoped selection requires an enrolled computer.
  return /* HTML */ `<div class="note compact">
      <strong>选择 → 授权 → 本机安装</strong>
      <p>
        网站记录你的选择；发布方负责授权。拿到授权后，在所选电脑的 PowerShell
        中执行安装。
      </p>
    </div>
    ${state.products.length ? /* HTML */ `<div class="product-grid">${state.products.map(productCard).join("")}</div>` : empty("业务软件尚未上架", "当前目录为空。发布方准备好软件后会在此提供版本；你的电脑已接入，可以稍后回来刷新。")}
    <section class="panel update-guide">
      <h2>安装后，按需开启自动更新</h2>
      <p>
        在软件卡片中复制对应命令，显式启用该软件的自动更新。Windows
        用户登录期间，工具每 15 分钟检查一次；终端可关闭。机器级软件需在管理员
        PowerShell 中安装和启用更新。
      </p>
      <p class="hint">
        连续离线 30 天后需重新登录。随时运行以下命令查看本机状态或立即同步：
      </p>
      ${command(cli("status"))}${command(cli("sync"))}
    </section>`;
}

function productCard(
  p,
) /* GATE: display prerequisite and recovery for unavailable actions */ {
  const release = p.releases.find((r) => r.target === "windows-x86_64");
  const ready = p.authorized && p.selected && release; // GATE: installation instructions require selection, permission and Windows release.
  const permission = p.authorized
    ? badge("已获授权", "success")
    : p.selected
      ? badge("等待发布方授权", "waiting")
      : badge("需要申请", "neutral");
  return /* HTML */ `<article class="panel product">
    <div class="section-title">
      <span class="product-monogram">${esc(p.label.slice(0, 1))}</span
      >${permission}
    </div>
    <h2>${esc(p.label)}</h2>
    <p>${esc(p.description || "发布方尚未提供软件说明。")}</p>
    <div class="product-meta">
      <code>${esc(p.id)}</code
      ><span
        >${release ? `Windows · ${esc(release.version)}` : "尚无 Windows 版本"}</span
      >
    </div>
    ${
      ready
        ? /* HTML */ `<div class="install-guide">
            <h3>在当前电脑上安装</h3>
            ${command(cli(`install ${productArg(p.id)}`))}
            <h3>可选：开启自动更新</h3>
            ${command(cli(`auto-update ${productArg(p.id)} enable`))}
            <p class="hint">
              安装后查看「安装记录」。自动更新需你主动执行开启命令。
            </p>
          </div>`
        : !p.selected
          ? /* HTML */ `<button
              class="button primary"
              data-product="${esc(p.id)}"
            >
              ${p.authorized ? "选择软件" : "申请使用"} <span>→</span>
            </button>`
          : /* HTML */ `<div class="subnote">
              ${!p.authorized ? "申请已提交。请联系发布方为这台电脑授权，然后刷新查看。" : "已选择软件。请联系发布方提供 Windows x86-64 版本，然后刷新查看。"}
            </div>`
    }
  </article>`; // GATE: unavailable permission/release shows actionable next step instead of install action.
}

function currentReceipts() {
  return state.deployments
    .filter((d) => d.device === state.device)
    .sort((a, b) => b.updated - a.updated);
}

function history() {
  const receipts = currentReceipts();
  return `${
    receipts.length
      ? /* HTML */ `<div class="receipts">
          ${receipts
            .map(
              (r) =>
                /* HTML */ `<article class="panel receipt">
                  <div class="receipt-head">
                    <div>
                      <span class="eyebrow">${esc(r.product)}</span>
                      <h2>
                        ${esc(state.products.find((p) => p.id === r.product)?.label || r.product)}
                        <small>${esc(r.version)}</small>
                      </h2>
                    </div>
                    ${badge(statuses[r.status] || r.status, r.status === "failed" ? "danger" : r.status === "installed" ? "success" : "waiting")}
                  </div>
                  <dl class="receipt-meta">
                    <div>
                      <dt>更新时间</dt>
                      <dd>${time(r.updated)}</dd>
                    </div>
                    <div>
                      <dt>自动更新</dt>
                      <dd>
                        ${r.detail.auto_update === true ? "已启用（最近回执）" : r.detail.auto_update === false ? "未启用（最近回执）" : "回执尚未报告，请在本机运行 status 查看"}
                      </dd>
                    </div>
                  </dl>
                  ${
                    r.detail.reason
                      ? /* HTML */ `<div class="error">
                          <strong>${esc(r.detail.reason)}</strong>
                          <p>
                            ${esc(r.detail.recovery || "请检查本机安装日志，并联系发布方确认恢复步骤。")}
                          </p>
                        </div>`
                      : ""
                  }${r.status === "restart_required" ? '<div class="subnote">请先保存工作，再自行重启 Windows。工具不会替你重启电脑。</div>' : ""}${r.status === "reserved" || r.status === "installing" ? '<p class="hint">等待电脑回传结果。若安装被中断，请先检查本机软件和日志，再联系发布方处理原操作。</p>' : ""}
                  <details>
                    <summary>查看交付详情</summary>
                    <dl class="metadata">
                      <dt>操作编号</dt>
                      <dd class="digest">${esc(r.id)}</dd>
                      <dt>电脑编号</dt>
                      <dd class="digest">${esc(r.device)}</dd>
                      <dt>创建时间</dt>
                      <dd>${time(r.created)}</dd>
                    </dl>
                    <pre>${esc(JSON.stringify(r.detail, null, 2))}</pre>
                  </details>
                </article>`,
            )
            .join("")}
        </div>`
      : empty(
          "还没有安装回执",
          state.device
            ? "授权后在电脑上执行安装命令，再刷新此页查看结果。"
            : "先接入电脑，再选择软件并在本机安装。",
          link(
            state.device ? "software" : "setup",
            state.device ? "前往软件目录" : "接入电脑",
          ),
        )
  }<div class="note"><strong>安装状态与软件运行分别确认</strong><p>「已安装」表示安装流程已完成。业务软件能否正常运行，需要在电脑上打开软件确认。更新结果同样以电脑回传的记录为准。</p></div>`;
}

async function refresh() {
  const [devices, downloads, deployments] = await Promise.all([
    api("devices"),
    api("downloads"),
    api("deployments"),
  ]); // GATE: authenticated data requests.
  state.devices = devices.devices;
  state.downloads = downloads.downloads;
  state.deployments = deployments.deployments;
  state.device = state.devices.some((d) => d.id === state.device)
    ? state.device
    : state.devices[0]?.id || "";
  state.products = state.device
    ? (await api(`catalog?device=${encodeURIComponent(state.device)}`)).products
    : []; // GATE: catalog scope follows authenticated device choice.
  shell();
}

async function perform(
  action,
  button,
) /* GATE: serialize each submitted action */ {
  const originalLabel = button?.innerHTML;
  if (button) {
    button.disabled = true; // GATE: wait for this request to finish before resubmitting.
    button.setAttribute("aria-busy", "true");
    button.textContent = "处理中…";
  } // GATE: prevent duplicate requests while operation is in progress.
  state.error = "";
  state.recovery = "";
  try {
    await action();
  } catch (error) {
    state.error =
      {
        session_expired: "登录已过期，请重新登录",
        login_required: "请先登录企业账号",
        credentials_rejected: "账号或密码不正确",
        login_rate_limited: "登录尝试过于频繁",
        software_permission_required: "这款软件尚未获得授权",
        device_login_required: "此操作需要在电脑上登录 GESAC",
        operation_unavailable: "当前操作暂不可用",
      }[error.message] || "暂时无法完成请求";
    state.recovery =
      {
        session_expired: "请重新登录企业账号，随后可继续当前页面。",
        login_required: "请使用企业账号登录，随后可继续当前页面。",
        login_rate_limited: "请等待一分钟后，再次输入企业账号与密码登录。",
        credentials_rejected:
          "请检查企业账号与密码；若忘记密码，请联系发布方。",
        software_permission_required:
          "请联系发布方为当前电脑授权，然后刷新软件目录。",
        device_login_required:
          "请在所选电脑的 PowerShell 中运行 .\\gesac.exe login，再执行本机命令。",
        operation_unavailable:
          "请刷新查看当前状态；若仍不可用，请将错误详情提供给发布方。",
      }[error.message] ||
      "请检查网络后刷新重试。若持续失败，请把下方错误详情提供给发布方。";
    state.errorDetail = [error.message, error.recovery]
      .filter(Boolean)
      .join(" · ");
    state.session ? shell() : login();
  } finally {
    if (button) {
      button.disabled = false;
      button.removeAttribute("aria-busy");
      button.innerHTML = originalLabel;
    }
  }
}

app.addEventListener("submit", (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  perform(async () => {
    await api("session/login", {
      method: "POST",
      body: JSON.stringify({
        account: form.get("account"),
        password: form.get("password"),
      }),
    }); // GATE: account credentials authorize session creation.
    state.session = await api("session"); // GATE: cookie session is checked by service.
    await refresh();
  }, event.submitter);
});
app.addEventListener("change", (event) => {
  if (event.target.id === "device") {
    state.device = event.target.value;
    perform(async () => {
      state.products = state.device
        ? (await api(`catalog?device=${encodeURIComponent(state.device)}`))
            .products
        : [];
      shell();
    }); // GATE: load the selected account-owned device catalog.
  }
});
app.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  if (button.dataset.copy) {
    navigator.clipboard.writeText(button.dataset.copy).then(
      () => {
        document.querySelector("#notice").textContent =
          "命令已复制，请在所选电脑的 PowerShell 中运行。";
      },
      () => {
        document.querySelector("#notice").textContent =
          "浏览器未允许复制，请选中命令文字并手动复制。";
      },
    );
  }
  if (button.dataset.action === "download")
    perform(async () => {
      const file = await api("downloads/windows-x86_64", {}, true); // GATE: session required to download the published client.
      const url = URL.createObjectURL(file);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "gesac.exe";
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    }, button);
  if (button.dataset.action === "refresh")
    perform(async () => {
      if (!state.session) /* GATE: retry requires login */ {
        state.session = await api("session"); // GATE: service validates the browser session.
      }
      await refresh();
    }, button); // GATE: retry requires a valid browser session.
  if (button.dataset.action === "logout")
    perform(async () => {
      await api("session", { method: "DELETE" });
      state.session = null;
      state.device = "";
      state.products = [];
      state.deployments = [];
      state.devices = [];
      login();
    }, button); // GATE: logout ends browser access.
  if (button.dataset.product)
    perform(async () => {
      const result = await api("selections", {
        method: "POST",
        body: JSON.stringify({
          product: button.dataset.product,
          device: state.device,
        }),
      });
      await refresh();
      document.querySelector("#notice").textContent =
        result.status === "ready"
          ? "软件已选择，可在电脑上执行安装命令。"
          : "申请已提交，请等待发布方授权。";
    }, button); // GATE: publisher controls authorization after selection.
});
window.addEventListener("hashchange", () => {
  if (location.hash === "#main") {
    document.querySelector("#main").focus();
    return;
  }
  state.page = pages[location.hash.slice(1)]
    ? location.hash.slice(1)
    : "overview";
  if (state.session) {
    shell();
    document.querySelector("#main").focus();
  }
});
perform(async () => {
  try {
    state.session = await api("session"); // GATE: service validates the browser session.
  } catch (error) {
    // GATE: session restoration requires valid cookie.
    if (error.status === 401) /* GATE: login is required */ {
      login();
      return;
    }
    throw error;
  } // GATE: first unauthenticated visit starts with login.
  await refresh();
});
