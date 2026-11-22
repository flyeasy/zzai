# 代码签名与系统信任（字在 / zzai）

> 目的：把「Windows 蓝屏警告 SmartScreen」「从互联网下载的软件不可信」「macOS 无法验证开发者」这三件事讲清楚，
> 给出「今天不花钱能做的」和「花钱能彻底消除的」两条线，以及每条线的成本与前置条件。

## 1. 现在为什么会被拦

| 现象 | 真正的原因 | 谁在拦 |
| --- | --- | --- |
| 双击 exe 出现蓝色「Windows 已保护你的电脑 / 未知发布者」 | exe 没有 Authenticode 数字签名（或签名证书没有信誉） | SmartScreen（基于文件哈希 + 证书 + 下载量的信誉云） |
| 右键属性里多出「此文件来自其他计算机，可能被阻止…解除锁定」 | zip/exe 带 `Zone.Identifier`（Mark-of-the-Web），从浏览器/邮件下载就有 | Windows 附件管理器（Attachment Manager） |
| 解压后双击 bat，PowerShell 报「禁止运行脚本」 | 执行策略 RemoteSigned + 脚本同样被标记为来自互联网 | PowerShell 执行策略 |
| mac 上「无法打开，因为无法验证开发者」 | 没有 Developer ID 签名 / 没做公证（notarization） | Gatekeeper |

**关键认知**：签名解决「身份可信」，信誉解决「这个文件安全吗」。两者都由微软云判定，**新签名证书 = 零信誉**，所以签完仍可能被拦一段时间（EV 证书以前免拦，2023 年后微软也改为「更快积累」而不是立刻放行）。

## 2. 三条路线（按「彻底程度」排）

### A. 不签名，靠说明 + 校验（今天就能做，0 成本）
- 安装包里 README 第一屏写清「点更多信息 → 仍要运行」「右键 → 解除锁定」；官网下载页同样放一段（带截图）。
- 站点公布 `SHA256SUMS.txt`，用户可自行核对：`Get-FileHash zzai-0.8.1-win64.zip -Algorithm SHA256`。
- 提供免安装 zip（解压即用、`portable.txt` 便携模式），减少「安装器」带来的额外拦截面。
- 局限：警告**不会消失**；只能减少用户困惑。适合内测/早期分发。

### B. Apple Developer Program + 公证（macOS 唯一正解，$99/年）
- 前置：Apple ID + 付费加入 Developer Program（个人即可，审核 1–2 天）。
- 流程：`codesign --deep --options runtime --sign "Developer ID Application: …"` → `xcrun notarytool submit … --wait` → `xcrun stapler staple`。
- 效果：mac 上「无法验证开发者」彻底消失，可正常双击。
- 成本：$99/年；工作量约半天（脚本化后每次发版多 2–5 分钟）。

### C. Windows 代码签名证书（消除「未知发布者」，但 SmartScreen 需攒信誉）
| 方案 | 价格 | 前置条件 | 备注 |
| --- | --- | --- | --- |
| **Azure Trusted Signing** | $9.99/月 | 组织（公司/个体户）+ 至少 3 年可验证经营历史 + 组织身份验证 | 微软自家服务，密钥在云 HSM，最省心；个人开发者暂不符合 |
| **OV 代码签名证书**（Sectigo/DigiCert/SSL.com/Certum 等） | 约 $100–250/年 | 个人或组织身份验证（身份证/营业执照 + 电话核验） | 2023 年 6 月起必须硬件令牌或云签名（SSL.com eSigner / Certum cloud），不能只拿 .pfx |
| **Certum Open Source 证书** | 约 €30–60/年 | 开源项目（公开仓库 + 许可证） | 最便宜的正规路径；要求项目开源 |
| **EV 代码签名** | $300–500/年 | 组织 + 更严的核验 | 不再「免 SmartScreen」，只是信誉积累更快 |
| **Microsoft Store（MSIX 打包）** | $19 一次性（个人开发者账号） | 把 Win32 应用打成 MSIX 提交审核 | 由微软签名，**用户端完全无 SmartScreen**；代价是要过商店审核 + 打包改造（本应用用到低级键盘钩子/UI Automation，需要 `runFullTrust` 能力，可行但要做一次适配） |

**签完还要做的事**：每次发布用 `signtool sign /fd SHA256 /tr <时间戳服务器> /td SHA256`；时间戳必须打，否则证书过期后旧安装包全部失效。

## 3. 建议（按你的实际情况选一条）

1. **如果你有公司/个体户，且打算长期做** → Azure Trusted Signing（$9.99/月）+ Apple $99/年。
   一次性投入小、全自动、密钥不落地；Windows 侧仍要 2–8 周攒信誉，但至少显示「发布者：XX」。
2. **如果没有经营主体、也不想注册公司** → Certum Open Source（€30/年）+ Apple $99/年；或者干脆先走 A（说明 + SHA256），把预算留到有下载量之后。
3. **如果目标是「用户零提示」且不想买证书** → Microsoft Store 上架（$19），但它要求过审 + 打包，属于产品决策。
4. 内测阶段：A 已经够用；等要对外推广再上 B/C。

## 4. 今天已经落地的部分

- 安装包 `README.txt` 第一屏增加了「第一次运行时 Windows 的两种提示」：SmartScreen 怎么过、Mark-of-the-Web 怎么解除（含 `Unblock-File` 一行命令）。
- 官网 `dl/SHA256SUMS.txt` 公布两个包的哈希（Windows / macOS），可对照 `Get-FileHash` / `shasum -a 256`。
- 本文档作为后续决策依据；具体采购/注册步骤见第 5 节。

## 5. 真要动手时的操作清单

### Windows（以 Azure Trusted Signing 为例）
1. 用组织账号登录 Azure → 创建 `Trusted Signing` 资源（区域选 East US）。
2. 完成 Identity Validation（组织：营业执照 + 电话/邮件核验；约 1–3 个工作日）。
3. 创建 Certificate Profile（类型 `Public Trust`），拿到 endpoint / account / profile 名。
4. CI 里用 `trusted-signing-cli` 或 `signtool` 的 `/dlib` 插件签名：
   `signtool sign /v /fd SHA256 /tr http://timestamp.acs.microsoft.com /td SHA256 /dlib <AzureSignTool dlib> /dmdf metadata.json Eryu.exe`
5. 打包顺序：先 `cargo xwin build` → 签 `Eryu.exe` → 再 `package-win.sh`（保证 zip 内是签过名的 exe）。

### Windows（OV 证书 + 云签名）
1. 在 SSL.com / Certum 购买 OV 代码签名，提交身份材料，等待签发（1–5 天）。
2. 用厂商的云签名工具（eSigner / Certum Cloud）或硬件令牌；`signtool` 加上 `/csp`/`/kc` 或厂商 CLI。
3. 同样先签 exe 再打包，所有分发物（exe、msi）都要签。

### macOS
1. 加入 Apple Developer Program → 生成 `Developer ID Application` 证书并导入钥匙串。
2. `codesign --force --deep --options runtime --timestamp --sign "Developer ID Application: …" Eryu.app`
3. `ditto -c -k --keepParent Eryu.app Eryu.zip` → `xcrun notarytool submit Eryu.zip --keychain-profile zzai --wait`
4. `xcrun stapler staple Eryu.app` → 再打最终 zip（公证票要随包）。

## 6. 未做/待定

- 采购主体（个人 / 个体户 / 公司）未定 → 决定走 Azure 还是 Certum。
- 是否上 Microsoft Store 未定（需要一次 MSIX 打包改造）。
- 签名接入 CI 后，`scripts/build-win.sh` / `scripts/dist.sh` 需要加签名步骤（当前没有）。
