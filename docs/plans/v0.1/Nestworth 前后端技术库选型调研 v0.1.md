# Nestworth 前后端技术库选型调研

> 日期：2026-08-17  
> 技术基线：macOS + Tauri 2 + Rust + TypeScript + SQLite

## 1. 推荐技术栈总览

我建议 Nestworth 采用：

```text
Nestworth
│
├── Desktop
│   └── Tauri 2
│
├── Frontend
│   ├── React
│   ├── TypeScript
│   ├── Vite
│   ├── Tailwind CSS
│   ├── shadcn/ui + Base UI
│   ├── Lucide
│   ├── Recharts
│   ├── TanStack Router
│   ├── TanStack Query
│   ├── TanStack Table
│   ├── React Hook Form
│   ├── Zod
│   └── react-i18next
│
└── Rust
    ├── Tauri
    ├── SQLx + SQLite
    ├── rust_decimal
    ├── Serde
    ├── UUID
    ├── Chrono
    ├── Reqwest
    ├── thiserror
    └── tracing
```

整体原则是：**React 负责表现和交互，Rust 负责数据库、金额计算、估值、投资收益、行情 API 和数据完整性。**

---

# 2. Frontend

## React + TypeScript

**推荐：React**

Tauri 本身不限制前端框架，并明确推荐 SPA 类型项目使用 Vite；React 官方也提供 Vite + TypeScript 的直接初始化方案。Nestworth 没有 SSR、SEO 或服务端 React 的需求，因此没有必要引入 Next.js。

推荐：

```text
react
react-dom
typescript
vite
@vitejs/plugin-react
```

---

# 3. CSS / UI

## Tailwind CSS 4

**推荐。**

Tailwind 4 已提供官方 Vite Plugin，和当前 Vite/React 组合非常自然。

推荐：

```text
tailwindcss
@tailwindcss/vite
```

Nestworth 会有大量：

- Dashboard
- Sidebar
- Table
- Card
- Form
- Dialog
- Badge
- Filter
- Account Detail

Tailwind 比自己维护大量 CSS Modules 更适合。

---

## shadcn/ui

**强烈推荐。**

它和传统 Component Library 不太一样：组件代码直接进入项目，因此可以自己修改。这一点对 Nestworth 很重要，因为最终应该做出明显的 **macOS Desktop App 风格**，而不是看起来像普通 Web Admin。

建议：

```text
shadcn/ui
+
Base UI primitives
```

目前 shadcn 已支持 Base UI、React Aria、Radix 等 primitives；Base UI 本身是 unstyled、accessible 的 React primitives，非常适合自行构建桌面风格。

例如可以直接使用：

```text
Button
Dialog
Dropdown Menu
Context Menu
Popover
Tooltip
Tabs
Select
Input
Checkbox
Switch
Command
Card
Table
Sheet
Separator
```

### 不推荐

暂时不建议：

```text
Material UI
Ant Design
Chakra UI
```

不是这些库不好，而是它们的 Design Language 较强，之后要做成符合 Nestworth 自身视觉语言的 macOS App，反而需要覆盖大量默认样式。

---

# 4. 图标

## Lucide

**推荐：`lucide-react`**

Lucide 提供独立 React SVG Component，同时支持 tree shaking，目前覆盖大量 Finance、Charts、Accounts、Navigation 等图标。

```text
lucide-react
```

它也与 shadcn/ui 的生态非常一致。

适合：

```text
Wallet
Landmark
CreditCard
House
ChartLine
Bitcoin
CircleDollarSign
ArrowRightLeft
RefreshCw
Archive
Settings
```

品牌 Logo，例如：

```text
DBS
MooMoo
WeChat
Alipay
NASDAQ
```

则单独维护自己的 Institution Logo Asset，不应该从 Lucide 获取。

---

# 5. 图表

## Recharts

**推荐：Recharts。**

当前 Recharts 是基于 React Component 的组合式图表库；shadcn/ui 官方 Chart Component 本身也直接建立在 Recharts 之上，因此二者组合很自然。

推荐：

```text
recharts
```

可以覆盖 Nestworth v0.1.x 几乎所有需求：

```text
Net Worth → Line / Area

Assets vs Liabilities → Line

Asset Allocation → Pie / Donut

Currency Allocation → Bar

Investment Performance → Line

Cash Flow → Bar

Gain Attribution → Bar
```

### 暂时没必要用 ECharts

ECharts 更强，但 Nestworth 当前并没有：

- 数十万数据点
- Heatmap
- 3D
- Geographic Map
- 专业 Trading Chart

这类需求。

如果以后增加专业证券 K-Line，再单独引入 TradingView Lightweight Charts 一类工具即可。

---

# 6. Routing

## TanStack Router

推荐：

```text
@tanstack/react-router
```

Nestworth 最终会有明确页面层级：

```text
/
accounts
accounts/:id
investments
activity
analytics
automation
settings
```

TanStack Router 的优势是路由参数、Search Params 和 Navigation 都可以保持 TypeScript 类型安全。

这比自己写：

```ts
setCurrentPage("account")
```

长期更容易维护。

---

# 7. Rust 数据读取状态

## TanStack Query

我比较推荐：

```text
@tanstack/react-query
```

虽然 Nestworth 没有传统 HTTP Backend，但：

```text
React
 ↓
Tauri invoke()
 ↓
Rust
```

本质依然是异步数据源。

TanStack Query 可以统一处理：

```text
Loading
Error
Cache
Invalidate
Refetch
Mutation
```

它本身适用于任何返回 Promise 的异步数据源，并不要求一定是 HTTP。

例如：

```text
Account 更新成功

→ invalidate ["accounts"]
→ invalidate ["net-worth"]
→ invalidate ["allocation"]
```

非常适合 Nestworth。

---

# 8. Global State

## Zustand

**可选，不建议一开始大量使用。**

Zustand 本身定位就是轻量 React State Manager。

可以用于：

```text
Sidebar state
Selected household
UI preferences
Command palette
Temporary filters
```

但应该遵循：

```text
Rust / SQLite data
        ↓
TanStack Query

UI ephemeral state
        ↓
React state / Zustand
```

不要把：

```text
Accounts
Holdings
Transactions
Net Worth
```

复制进 Zustand。

因此甚至可以等 v0.1.2/v0.1.3 再决定是否真的需要 Zustand。

---

# 9. Form

Nestworth 会有大量复杂 Form：

```text
Account
Member
Group
Institution
Holding
Transfer
Buy / Sell
Automation
```

因此推荐组合：

```text
react-hook-form
zod
```

React Hook Form 专门负责表单状态和验证集成；Zod 是 TypeScript-first runtime schema validation。

例如：

```text
TransferForm

sourceAccount
destinationAccount
sourceAmount
destinationAmount
fxRate
fee
feeCurrency
date
note
```

这类表单使用 RHF 会比大量 `useState()` 更干净。

但：

> **Zod 只负责 Frontend Input Validation，真正的金融业务约束仍然必须由 Rust 再验证一次。**

不能相信 WebView 提交的数据。

---

# 10. i18n

## i18next + react-i18next

推荐：

```text
i18next
react-i18next
```

react-i18next 是 i18next 针对 React 的官方集成，并提供 `useTranslation()` 等 React API。

目录可以：

```text
src/locales/

en/
  common.json
  account.json
  investment.json

zh-CN/
  common.json
  account.json
  investment.json
```

例如：

```json
{
  "account.category.stock": "Stock"
}
```

对应：

```json
{
  "account.category.stock": "股票"
}
```

---

# 11. Currency / Number / Date Formatting

Frontend 不需要为了格式化金额再引入大型库。

优先使用浏览器原生：

```text
Intl.NumberFormat
Intl.DateTimeFormat
```

例如统一封装：

```text
formatMoney()
formatPercentage()
formatDate()
formatQuantity()
```

非常重要的是：

> **Frontend 不应该使用 JavaScript `number` 作为 Nestworth 的 authoritative financial value。**

金额应该由 Rust 以 Decimal 计算，通过 IPC 使用字符串传递，例如：

```json
{
  "value": "14490.000000"
}
```

Frontend 最终显示时再格式化。

---

# 12. Data Table

## TanStack Table

建议从 v0.1.2 / v0.1.3 开始加入：

```text
@tanstack/react-table
```

它是 headless table engine，可以自己控制 HTML 和样式，同时提供 Sorting、Filtering、Grouping、Column Visibility、Selection 等能力。

适合：

```text
Accounts
Holdings
Activity
Transactions
Quotes
```

尤其后期：

```text
Holding
Symbol
Quantity
Price
Value
Cost
Gain
Return
```

会明显比普通 `<table>` 更容易扩展。

---

# 13. Command Palette / Toast

可以直接通过 shadcn/ui 使用：

```text
cmdk
sonner
```

`cmdk` 专门用于构建 `⌘K` Command Menu，并且支持过滤、排序以及 accessible combobox。

未来：

```text
⌘K

Add Account
Transfer
Buy QQQ
Update DBS
Refresh Prices
Open MooMoo SG
```

会非常适合 macOS。

Toast 则用于：

```text
Prices updated
Account saved
Backup completed
Import failed
```

---

# 14. Backend — SQLite

## SQLx

我的推荐仍然是：

```text
sqlx
```

Feature 大致：

```text
sqlite
runtime-tokio
migrate
```

SQLx 原生支持 SQLite，并且自带 Migration 系统，可以把 migration 编译进程序。

适合维护：

```text
001_initial.sql
002_portfolio.sql
003_activity.sql
004_analytics.sql
005_automation.sql
```

### 一个非常重要的注意点

SQLx **明确没有给 SQLite 实现 rust_decimal Decimal 映射**，因为 SQLite 自身没有真正的 arbitrary/fixed precision decimal 类型。

因此建议继续采用之前确定的：

```text
SQLite
TEXT

"14490.000000"
```

Repository 层：

```text
TEXT
 ↓
Decimal::from_str()
 ↓
Domain
```

而不是：

```text
SQLite REAL
```

这也是 Nestworth 最值得一开始就定死的技术约束之一。

---

# 15. 金融 Decimal

## rust_decimal

**必须。**

```text
rust_decimal
```

它提供固定精度 Decimal，并明确面向需要避免浮点 round-off error 的金融计算场景。

用于：

```text
Money
Quantity
Price
FX
Cost Basis
Gain
Return calculation inputs
```

例如：

```rust
struct Money {
    amount: Decimal,
    currency: Currency,
}
```

这是 Backend 最重要的基础库之一。

---

# 16. Serialization

## Serde

必选：

```text
serde
serde_json
```

Serde 是 Rust 的通用 Serialization / Deserialization Framework。

承担：

```text
Rust Domain DTO
        ↕
Tauri IPC
        ↕
TypeScript
```

建议统一：

```rust
#[serde(rename_all = "camelCase")]
```

使 TypeScript DTO 保持：

```text
accountId
baseCurrency
createdAt
```

而 Rust 内部继续：

```text
account_id
base_currency
created_at
```

Serde 官方本身支持字段 rename。

---

# 17. ID

## uuid

推荐：

```text
uuid
```

用于：

```text
HouseholdId
MemberId
AccountId
InstrumentId
ActivityId
```

UUID 不需要中心分配器即可生成唯一标识，非常适合以后可能出现的 Import / Sync。

Domain 层最好进一步包装：

```text
AccountId(Uuid)
MemberId(Uuid)
```

避免把不同 Entity ID 传错。

---

# 18. Date / Time

## Chrono

推荐：

```text
chrono
```

用于：

```text
created_at
updated_at
effective_at
quoted_at
opened_at
closed_at
```

Chrono 提供 timezone-aware DateTime，并支持 UTC Timestamp 等操作。

数据库建议保持：

```text
UTC
```

UI 再做 Locale 转换。

---

# 19. HTTP / Quote Provider

## Reqwest

推荐：

```text
reqwest
```

用于：

```text
Stock Price API
Fund API
FX API
Crypto API
```

Reqwest 提供 async HTTP Client、JSON、TLS 等常用 HTTP 能力。

应该创建：

```text
QuoteProvider
FxProvider
```

抽象，而不是把 Reqwest 调用散落在 Service 中。

同时复用：

```text
reqwest::Client
```

而不是每次创建一个新 Client；Reqwest 文档也明确提示高频请求应复用 Client。

---

# 20. Error Handling

## thiserror

推荐：

```text
thiserror
```

非常适合 Domain Error：

```rust
AccountError
TransferError
ValuationError
QuoteError
DatabaseError
```

它提供标准 `std::error::Error` derive。

例如最终 Tauri IPC 可以统一转换成：

```text
AppError
 ├── Validation
 ├── NotFound
 ├── Database
 ├── Network
 └── Internal
```

---

# 21. Logging

推荐：

```text
tracing
tracing-subscriber
tauri-plugin-log
```

`tracing` 提供 structured、event-based instrumentation，比较适合 async Rust 应用。

Tauri 官方 Log Plugin 当前也可以与 tracing 系统集成。

以后调试：

```text
quote.refresh
fx.refresh
db.migration
activity.create
valuation.snapshot
backup.create
```

会比散落的：

```rust
println!()
```

强很多。

---

# 22. 推荐 Tauri Plugins

| Plugin | 使用阶段 | 用途 |
|---|---|---|
| `tauri-plugin-dialog` | v0.1.1 | Avatar / Import / Export 文件选择 |
| `tauri-plugin-window-state` | v0.1.1 | 保存窗口尺寸和位置 |
| `tauri-plugin-log` | v0.1.1 | 日志 |
| `tauri-plugin-opener` | v0.1.5 | 打开导出文件、网页 |
| `tauri-plugin-updater` | 发布阶段 | 自动更新 |

Tauri 的 Dialog Plugin 提供原生 File/Open/Save Dialog；Window State Plugin 可以自动恢复窗口尺寸与位置。

Updater 官方支持静态 JSON 或更新服务器，适合以后正式发布版本。

---

# 23. 不建议使用 tauri-plugin-sql

Nestworth 不建议：

```text
TypeScript
 ↓
tauri-plugin-sql
 ↓
SQLite
```

而坚持：

```text
TypeScript
 ↓
Tauri Command
 ↓
Rust Application Service
 ↓
Repository
 ↓
SQLite
```

因为：

```text
Transfer
Buy
Sell
Valuation
Cost Basis
FX
Net Worth
```

都属于 Domain Logic，而不是简单 CRUD。

这样前端永远不知道数据库 Schema。

---

# 24. 也不建议使用 tauri-plugin-store 保存业务数据

Tauri 官方 Store Plugin 是 Persistent Key-Value Store。

Nestworth 已经有 SQLite，因此没有必要再产生：

```text
SQLite
+
Store
+
localStorage
```

三个 Persistent State Source。

原则应该是：

```text
Financial / Settings Data
→ SQLite

Temporary UI State
→ Memory
```

这样最简单。

---

# 25. Import / Export

到 v0.1.5 再加入：

```text
csv
zip
```

Rust 负责：

```text
CSV Import
CSV Export

.nestworth Backup
ZIP Packaging
Restore
```

文件内容不建议交给 JavaScript 操作。

Tauri 官方也明确说明，Backend 可以直接使用 Rust 的 `std::fs`、`tokio::fs` 等文件操作，因此不需要为了 Rust 文件 I/O 再通过 Frontend FS Plugin 绕一层。

---

# 26. Testing

## Frontend

推荐：

```text
vitest
@testing-library/react
@testing-library/user-event
```

Vitest 与 Vite 使用同一套 transform/config pipeline，非常适合当前技术栈；React Testing Library 则强调通过用户实际看到和操作的 DOM 来测试组件。

重点测试：

```text
Account Form
Transfer Form
Holding Form
Filters
Navigation
Charts data mapping
```

---

## Rust

首先大量使用：

```text
cargo test
```

另外我建议增加：

```text
proptest
```

用于金融 invariant。

例如自动生成不同金额验证：

```text
Internal transfer
→ Net Worth unchanged

Buy stock excluding fee
→ Net Worth unchanged at execution price

Asset - Liability
→ Net Worth

Ownership total
→ 100%
```

Proptest 可以自动生成输入并进行 property-based testing。

这个对 Nestworth 的价值会明显高于普通应用。

---

# 27. 最终推荐 package map

## Frontend P0

```text
react
react-dom
typescript
vite

tailwindcss
@tailwindcss/vite

shadcn/ui
@base-ui/react

lucide-react

@tanstack/react-router
@tanstack/react-query

react-hook-form
zod

i18next
react-i18next

recharts
```

## Frontend P1

```text
@tanstack/react-table

zustand

cmdk
sonner

vitest
@testing-library/react
@testing-library/user-event
```

## Rust P0

```text
tauri

serde
serde_json

sqlx

rust_decimal

uuid
chrono

thiserror

tracing
tracing-subscriber
```

## Rust P1

```text
reqwest

csv
zip

proptest
```

## Tauri Plugins

```text
tauri-plugin-dialog
tauri-plugin-window-state
tauri-plugin-log

tauri-plugin-opener
tauri-plugin-updater
```

---

# 28. 我最终建议的组合

如果现在直接开始 `v0.1.1`，我不会一次把上面所有东西全部安装。

第一批只安装：

```text
React
Vite
TypeScript

Tailwind 4
shadcn/ui
Base UI
Lucide

TanStack Router
TanStack Query

React Hook Form
Zod

i18next
react-i18next

Tauri

SQLx
rust_decimal
Serde
UUID
Chrono
thiserror
tracing

tauri-plugin-dialog
tauri-plugin-window-state
tauri-plugin-log
```

然后：

```text
v0.1.2
→ Recharts
→ TanStack Table
→ Reqwest

v0.1.3
→ Activity相关无需新增大型依赖

v0.1.4
→ Analytics继续使用Recharts

v0.1.5
→ cmdk
→ csv
→ zip
→ updater / opener
```

我认为这是一个比较克制的依赖组合。

尤其应该坚持三件事：

**第一，SQLite 只由 Rust 访问。**

**第二，所有 Money / Price / Quantity / FX 核心计算使用 `rust_decimal`，IPC 金额使用字符串而不是 JavaScript number。** `rust_decimal` 正是为避免金融计算中的浮点舍入问题而设计，而 SQLx 也明确提醒 SQLite 本身不存在真正的 Decimal 映射。

**第三，前端把 `TanStack Query` 当作 Rust Domain State 的 View Cache，而不是再复制一套业务状态到 Zustand。**

按这个组合开发，Nestworth 的架构会比较清晰：

```text
React UI
   │
   ├── React Hook Form + Zod
   ├── TanStack Query
   ├── Recharts
   └── shadcn/ui
           │
       Tauri IPC
           │
      Rust Services
           │
   ┌───────┴────────┐
   │                │
SQLx/SQLite     Reqwest
   │                │
Financial       Quote / FX
Domain          Providers
   │
rust_decimal
```

这套我认为已经可以作为 Nestworth `v0.1.x` 的正式技术基线。