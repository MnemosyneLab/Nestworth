# Nestworth v0.1.x 产品路线图与分版本 PRD

> **Product:** Nestworth  
> **Positioning:** Local-first personal & household net worth tracker  
> **Platform:** macOS  
> **Desktop Framework:** Tauri  
> **Backend:** Rust  
> **Frontend:** TypeScript  
> **Storage:** SQLite  
> **Release Line:** v0.1.x

---

# 1. 产品目标

Nestworth 是一款面向个人和家庭的本地优先资产负债管理工具。

它不试图替代传统记账 App，而主要解决以下问题：

- 家庭目前拥有多少资产？
- 当前有多少负债？
- 净资产是多少？
- 钱分别在哪里？
- 每项资产属于哪个家庭成员？
- 不同类别、机构、Group、币种的资产分别有多少？
- 股票、基金、Crypto 等投资当前价值是多少？
- 净资产在过去一段时间如何变化？
- 净资产变化来自存款、投资收益、汇率还是其他因素？
- 投资实际收益如何？
- 是否有长期没有更新的资产？
- 是否能够自动记录工资、定投、贷款还款等规律性变化？

Nestworth 的核心边界：

> **Track wealth, not every expense.**

---

# 2. v0.1.x 总体路线

```text
v0.1.1
Foundation
家庭资产负债表

        ↓

v0.1.2
Valuation
多币种 + 投资持仓 + 行情

        ↓

v0.1.3
Ledger
资产变动 + 转账 + 历史

        ↓

v0.1.4
Analytics
投资收益 + 净资产分析

        ↓

v0.1.5
Automation & Polish
自动记录 + 导入导出 + 完整 MVP
```

最终：

```text
v0.1.5 = Nestworth MVP
```

---

# 3. 技术架构原则

## 3.1 Local First

Nestworth 默认：

- 不需要登录
- 不依赖云端服务
- SQLite 是 Source of Truth
- 所有核心功能离线可用
- 行情 API 不可用时仍然可以手工维护数据

未来同步能力不能改变这一原则。

---

# 4. 应用架构

建议采用：

```text
┌─────────────────────────────────────┐
│              Tauri App              │
│                                     │
│  TypeScript Frontend                │
│                                     │
│  ┌───────────────────────────────┐  │
│  │ UI                            │  │
│  │ View Model / State            │  │
│  └───────────────┬───────────────┘  │
│                  │ Tauri IPC         │
│  ┌───────────────▼───────────────┐  │
│  │ Rust Application Layer        │  │
│  │                               │  │
│  │ AccountService                │  │
│  │ PortfolioService              │  │
│  │ ValuationService              │  │
│  │ ActivityService               │  │
│  │ AnalyticsService              │  │
│  └───────────────┬───────────────┘  │
│                  │                  │
│  ┌───────────────▼───────────────┐  │
│  │ Repository Layer              │  │
│  └───────────────┬───────────────┘  │
│                  │                  │
│             SQLite                 │
└─────────────────────────────────────┘
```

---

# 5. Rust / TypeScript 职责划分

## Rust

负责：

- SQLite
- Migration
- Domain Model
- 金融计算
- Decimal 运算
- 汇率转换
- Portfolio 估值
- 收益率计算
- 行情 API
- 数据导入导出
- Automation
- 数据完整性
- Backup

所有涉及金额的关键业务逻辑尽量放在 Rust。

---

## TypeScript

主要负责：

- UI
- Navigation
- Form
- Chart
- Table
- Filtering
- Local UI State
- i18n
- View Model

不建议：

```text
TypeScript
    ↓
直接操作 SQLite
```

而应统一通过：

```text
TypeScript
    ↓
Tauri Command
    ↓
Rust Service
    ↓
SQLite
```

---

# 6. 金额精度

禁止使用：

```text
f32
f64
JavaScript number
```

执行关键金融计算。

统一使用 Decimal。

例如：

```text
quantity = 3.127182
price = 701.23
fxRate = 6.913421
```

Rust 领域层使用 Decimal 类型。

SQLite 建议保存：

```text
TEXT decimal
```

例如：

```text
"701.230000"
```

而不是依赖 SQLite REAL。

---

# 7. ID

所有主要实体使用 UUID：

```text
household_id
member_id
institution_id
account_id
holding_id
instrument_id
activity_id
group_id
```

不要依赖 SQLite 自增 ID 作为业务 ID。

这样未来：

- Sync
- Import
- Merge
- Multi-device

会简单很多。

---

# 8. 时间

数据库统一保存：

```text
UTC timestamp
```

UI 根据用户 Locale / Time Zone 显示。

对于金融事件另外保留：

```text
effective_date
```

例如：

```text
工资实际到账：
2026-08-25

录入时间：
2026-08-27
```

两者必须区分。

---

# 9. 删除原则

核心财务实体默认采用：

```text
Soft Delete
Archive
```

不要轻易物理删除参与历史计算的数据。

---

# 10. i18n 原则

从 v0.1.1 就建立 i18n。

至少：

```text
English
简体中文
```

所有 Category 使用内部 Key：

```text
asset.cash
asset.bank_account
investment.stock
liability.credit_card
```

而不是直接保存：

```text
股票
信用卡
```

---

---

# v0.1.1 — Foundation

# 11. 版本定位

## Theme

> **Build the household balance sheet.**

v0.1.1 的目标不是做投资软件，而是让用户第一次可以真正把家庭资产放进 Nestworth。

完成以后，用户应该可以回答：

> “我们家庭现在净资产是多少？”

---

# 12. v0.1.1 核心目标

实现：

```text
Household
Member
Institution
Group
Account
Ownership
Category
Manual Balance
Manual Value
Net Worth
```

这一版本必须建立稳定的数据模型。

---

# 13. 用户故事

## US-001 创建家庭

作为用户：

> 我希望创建一个 Household，并设置家庭名称和主币种。

例如：

```text
Wang Family

Base Currency:
CNY
```

---

## US-002 创建成员

用户可以：

- 创建 Member
- 修改名称
- 设置头像
- 修改头像
- 排序
- Archive

例如：

```text
Walt
Spouse
```

---

## US-003 创建 Institution

例如：

```text
DBS
MooMoo SG
WeChat
Alipay
OCBC
UOB
```

字段：

```text
name
logo
country
note
```

---

## US-004 创建 Group

例如：

```text
Emergency Fund
Retirement
Singapore
China
Baby Fund
```

Group：

- 可以设置名称
- Icon
- Color
- Description
- 排序
- Archive

---

# 14. Account

v0.1.1 支持三种基础 Tracking Mode。

## BALANCE

适合：

- 现金
- 银行账户
- Digital Wallet
- 信用卡
- 贷款

例如：

```text
DBS Savings

¥100,000
```

---

## MANUAL_VALUE

适合：

- 房产
- 汽车
- 收藏品
- 应收账款

例如：

```text
Apartment

¥4,500,000
```

---

## HOLDINGS

v0.1.1 数据模型预留。

但完整 Holdings UI 放到：

```text
v0.1.2
```

---

# 15. Account 字段

```text
id

householdId
institutionId?

name

primaryCategory
secondaryCategory

trackingMode

defaultCurrency

groupId?

note
icon

includeInNetWorth
includeInInvestmentPortfolio
includeInLiquidAssets

openedAt?
closedAt?

archived

createdAt
updatedAt
```

---

# 16. Ownership

Account 不直接使用：

```text
memberId
```

而使用独立 Ownership。

Schema：

```text
AccountOwnership

accountId
memberId
percentage
```

例如：

```text
DBS Savings

Walt 100%
```

或者：

```text
Home

Walt    50%
Spouse  50%
```

要求：

```text
sum(percentage) = 100%
```

---

# 17. 一级分类

```text
Cash Equivalent
Investment
Property
Receivable
Liability
```

中文：

```text
流动资金
投资
固定资产
应收账款
负债
```

---

# 18. v0.1.1 二级分类

至少支持：

## Cash Equivalent

```text
Cash
Bank Account
Digital Wallet
Broker Cash
Other
```

## Investment

暂时：

```text
Investment Account
Other Investment
```

## Property

```text
Real Estate
Vehicle
Collectible
Other Property
```

## Receivable

```text
Loan Receivable
Other Receivable
```

## Liability

```text
Credit Card
Mortgage
Auto Loan
Consumer Loan
Personal Debt
Other Liability
```

---

# 19. Net Worth

公式：

```text
Net Worth

=

Included Assets

-

Included Liabilities
```

首页显示：

```text
NET WORTH

¥3,821,392

Assets
¥4,921,392

Liabilities
¥1,100,000
```

---

# 20. Overview

首页第一版包含：

## Net Worth Card

```text
Net Worth
Assets
Liabilities
```

## Asset Allocation

按：

```text
Category
Member
Institution
Group
```

统计。

## Accounts

显示：

```text
Walt

DBS                  ¥100,000
WeChat                 ¥3,200


Shared

Home                ¥4,000,000
Mortgage           -¥1,200,000
```

---

# 21. Sidebar

```text
Overview

Accounts
    All
    Walt
    Spouse
    Shared

Groups

Institutions

Settings
```

---

# 22. Account CRUD

支持：

```text
Create
Edit
Archive
Restore
```

Delete 可以存在，但默认放在危险操作区域。

---

# 23. Balance Update

Account Detail：

```text
DBS Multiplier

Balance
¥100,000

[ Update Balance ]
```

更新：

```text
¥100,000
↓
¥110,000
```

v0.1.1 暂时只记录当前值。

正式 Activity Ledger：

```text
v0.1.3
```

---

# 24. Settings

支持：

```text
Household Name
Base Currency
Language
Appearance
```

语言：

```text
English
简体中文
```

---

# 25. SQLite Schema — v0.1.1

主要 Table：

```text
households

members

institutions

groups

accounts

account_ownership

schema_migrations
```

为了避免未来重构，可以提前建立：

```text
instruments
holdings
```

但 UI 暂不暴露。

---

# 26. v0.1.1 非功能需求

### 数据

- Foreign Key 开启
- SQLite WAL
- 每次 Migration 可重复验证
- App Crash 不应造成半完成写入

### UX

Account 创建不超过：

```text
2 dialogs
```

添加简单账户最好能：

```text
< 30 seconds
```

完成。

---

# 27. v0.1.1 验收标准

用户能够：

- 创建 Household
- 创建多个 Member
- 设置头像
- 创建 Institution
- 创建 Group
- 创建资产 Account
- 创建 Liability
- 设置 Ownership
- 修改余额
- 修改估值
- Archive Account
- 按 Member 查看资产
- 按 Group 查看资产
- 按 Institution 查看资产
- 查看 Net Worth
- 查看 Assets / Liabilities
- 切换中英文

在全部账户使用 Household Base Currency 的情况下：

```text
Net Worth
```

必须计算正确。

---

# 28. v0.1.1 暂不实现

```text
多币种换算
股票
基金
Crypto
行情 API
Transfer
完整 Activity
历史走势图
收益分析
Automation
CSV Import
```

---

---

# v0.1.2 — Multi-Currency & Portfolio

# 29. 版本定位

## Theme

> **Know what everything is worth.**

这一阶段让 Nestworth 从简单资产列表升级为真正的：

> Multi-currency wealth tracker

并第一次支持投资组合。

---

# 30. v0.1.2 核心能力

```text
Multi Currency

FX

Instrument

Holding

Investment Account

Market Price

Quote Cache

Batch Refresh

Valuation Engine
```

---

# 31. Multi Currency

支持三级币种：

```text
Household Base Currency

Account Default Currency

Instrument Currency
```

例如：

```text
Household:
CNY

MooMoo SG:
SGD

QQQ:
USD

ES3:
SGD
```

---

# 32. FX

创建：

```text
fx_quotes
```

字段：

```text
baseCurrency
quoteCurrency
rate

provider
isManual

quotedAt
createdAt
```

例如：

```text
USD / CNY

6.9000
```

---

# 33. Cash Value

```text
1000 USD

USD/CNY:
6.9

Value:

¥6,900
```

---

# 34. Instrument

Instrument 表示投资标的。

字段：

```text
id

name
symbol

instrumentType

market
country

currency

provider?
providerSymbol?

isin?

logo

archived
```

---

# 35. Instrument Type

首版支持：

```text
Stock
ETF
Mutual Fund
Crypto
Bond
Precious Metal
Bank Investment Product
Other
```

---

# 36. Holding

字段：

```text
id

accountId
instrumentId

quantity

manualPrice?

note

archived
```

---

# 37. Investment Account

例如：

```text
MooMoo SG
```

Account：

```text
trackingMode = HOLDINGS
```

展示：

```text
MooMoo SG

Total
¥203,281

Cash
S$5,000

Holdings

QQQ
3 shares
$700
¥14,490

ES3
1000 shares
S$4.21
¥23,391
```

---

# 38. Cash Balance

投资账户不能只有一个：

```text
balance
```

建议加入：

```text
account_cash_balances
```

例如：

```text
MooMoo SG

SGD 5,000
USD 2,000
```

字段：

```text
accountId
currency
balance
```

---

# 39. Quote

创建：

```text
instrument_quotes
```

字段：

```text
instrumentId

price
currency

provider
isManual

quotedAt
createdAt
```

---

# 40. Quote Provider

定义 Rust Trait：

```text
QuoteProvider
```

逻辑能力：

```text
fetch_quote()
fetch_quotes()
search_instrument()
```

FX 同样定义：

```text
FxProvider
```

不要让具体 API Provider 逻辑进入 Account Service。

---

# 41. Manual Fallback

任何 Instrument 必须允许：

```text
Manual Price
```

任何汇率必须允许：

```text
Manual FX Rate
```

即使 API 不支持：

```text
中国基金
银行理财
特殊债券
```

Nestworth 仍然应该能完整工作。

---

# 42. Batch Refresh

Toolbar：

```text
Refresh All
```

支持：

```text
Refresh Prices

Refresh FX

Refresh Everything
```

应该执行去重。

例如三个账户都持有 QQQ：

```text
QQQ quote
```

只请求一次。

---

# 43. Data Freshness

行情展示：

```text
QQQ

$700.31

Updated
2 minutes ago
```

状态：

```text
Fresh
Delayed
Stale
Manual
Unavailable
```

---

# 44. Valuation Engine

创建统一：

```text
ValuationService
```

Cash：

```text
nativeValue × FX
```

Holding：

```text
quantity × price × FX
```

Liability：

```text
-(balance × FX)
```

Manual asset：

```text
manualValue × FX
```

所有 Overview 页面禁止自行写估值公式。

统一调用：

```text
ValuationService
```

---

# 45. Asset Allocation

增加：

```text
Currency
Country
Instrument Type
```

例如：

```text
By Currency

CNY      45%
USD      31%
SGD      20%
Others    4%
```

---

# 46. Investment UI

Sidebar 增加：

```text
Investments
```

页面：

```text
Portfolio Value

Holdings

Allocation

Accounts
```

暂时不计算收益。

只显示：

```text
Current Market Value
```

---

# 47. v0.1.2 SQLite 增量

新增：

```text
account_cash_balances

instruments

holdings

instrument_quotes

fx_quotes
```

---

# 48. v0.1.2 验收标准

必须支持如下案例：

```text
Base Currency:
CNY

MooMoo SG

Cash:
5000 SGD

QQQ:
3 × 700 USD

ES3:
1000 × 4 SGD
```

系统能够：

- 获取 / 手工设置 SGD/CNY
- 获取 / 手工设置 USD/CNY
- 获取 QQQ Price
- 获取 ES3 Price
- 计算每个 Holding
- 计算 Investment Account Total
- 汇总 Household Net Worth

价格或 API 失败不能阻止 App 启动。

---

# 49. v0.1.2 暂不实现

```text
Buy / Sell
Transfer
Cost Basis
Dividend
Historical Chart
Investment Return
Automation
```

---

---

# v0.1.3 — Activity & History

# 50. 版本定位

## Theme

> **Understand how wealth changes.**

从这一版开始，Nestworth 不仅知道：

```text
现在有什么
```

还知道：

```text
发生了什么
```

这是整个产品从 Snapshot Tracker 向真正 Wealth Ledger 转变的一步。

---

# 51. 核心能力

```text
Activity Ledger

Balance Change

Transfer

Deposit

Withdrawal

Buy

Sell

Fee

Dividend

Interest

Historical Quotes

Historical FX

Historical Net Worth

Net Worth Chart

Reconciliation
```

---

# 52. Activity

建立统一 Activity 模型。

Activity Type：

```text
BALANCE_ADJUSTMENT

DEPOSIT
WITHDRAWAL

TRANSFER

BUY
SELL

DIVIDEND
INTEREST

FEE
TAX

INCOME
EXPENSE

DEBT_DRAW
DEBT_REPAYMENT

MANUAL_VALUATION

OTHER
```

---

# 53. Activity Header

```text
id

householdId

type

effectiveAt

note

createdAt
updatedAt
```

---

# 54. Activity Entries

推荐：

```text
activity
    ↓
activity_entries
```

而不是每个 Activity Type 建独立 Table。

Entry 可以表示：

```text
Account
Currency
Amount

Holding
Quantity Change

Fee
```

这样 Transfer 可以天然表达为：

```text
Activity

Entry A:
DBS
-1000 SGD

Entry B:
MooMoo
+780 USD
```

---

# 55. Transfer

Transfer Form：

```text
From

To

Source Amount

Destination Amount

FX Rate

Fee

Date

Note
```

---

# 56. Internal Transfer

需要明确：

```text
Internal Flow
```

不算作：

```text
Income
Expense
Investment Return
```

例如：

```text
DBS
-¥10,000

WeChat
+¥10,000

Net Worth:
0 change
```

---

# 57. Credit Card Payment

表达为：

```text
Cash
-¥5,000

Credit Card Liability
-¥5,000 liability
```

Net Worth：

```text
0
```

---

# 58. Buy

例如：

```text
Buy QQQ

3 shares
$700
Fee $2
```

系统产生：

```text
Cash

-$2,102

QQQ

+3 shares
```

净资产只因：

```text
fee
spread
market movement
```

发生变化。

---

# 59. Sell

支持：

```text
Quantity
Executed Price
Fee
Currency
Date
```

v0.1.3 可以保存 Cost 信息，但完整收益计算留到：

```text
v0.1.4
```

---

# 60. Balance Reconciliation

例如当前：

```text
DBS

¥100,000
```

用户输入：

```text
¥93,000
```

Nestworth 计算：

```text
Difference:

-¥7,000
```

询问：

```text
Record as

○ Balance Adjustment
○ Expense
○ Transfer
○ Other
```

默认：

```text
Balance Adjustment
```

---

# 61. Unclassified Change

如果用户选择：

```text
Balance Adjustment
```

Analytics 中标记：

```text
Unclassified Change
```

避免假装知道它属于：

```text
消费
投资亏损
转账
```

---

# 62. Historical Quote

v0.1.2 的 Quote 不再只是覆盖当前值。

从 v0.1.3 起完整保存：

```text
instrument_quotes
fx_quotes
```

历史记录。

---

# 63. Daily Snapshot

增加：

```text
valuation_snapshots
```

建议保存：

```text
Household
Account
Holding
```

至少 Household / Account 层保存每日 Snapshot。

不要依赖今天重新计算五年前的 Net Worth。

---

# 64. Snapshot Timing

触发：

```text
App launch

Activity completed

Quote refresh

Manual valuation update
```

每天同一 Entity 可以覆盖当日最新 Snapshot。

---

# 65. Net Worth Trend

首页增加：

```text
Net Worth Chart
```

范围：

```text
1M
3M
6M
YTD
1Y
3Y
5Y
ALL
```

Lines：

```text
Net Worth
Assets
Liabilities
```

---

# 66. Activity Page

Sidebar：

```text
Activity
```

展示：

```text
Today

Salary
DBS
+S$8,000

Transfer
DBS → MooMoo
S$2,000

QQQ
Bought 1
$700
```

---

# 67. Filter

支持：

```text
Date

Member

Account

Institution

Type

Instrument
```

---

# 68. Account Timeline

Account Detail：

```text
Overview

Holdings

Activity

Notes
```

---

# 69. Undo / Correction

财务 Activity 不建议直接静默覆盖。

第一版至少允许：

```text
Edit
Delete
```

但数据库应记录：

```text
updatedAt
```

未来可以升级完整 Audit Log。

---

# 70. v0.1.3 SQLite 增量

新增：

```text
activities

activity_entries

valuation_snapshots
```

Quote Table 转为完整 append-only history。

---

# 71. v0.1.3 验收标准

必须正确处理：

### Case 1

```text
DBS → WeChat
¥10,000
```

Net Worth 不变。

### Case 2

```text
DBS SGD → IBKR USD
```

记录实际 FX 和 Fee。

### Case 3

```text
Cash → QQQ
```

买入本身不会产生虚假的 Net Worth 增长。

### Case 4

```text
Credit Card Repayment
```

资产下降和负债下降互相抵消。

### Case 5

能够显示：

```text
过去一个月净资产走势图
```

---

# 72. v0.1.3 暂不实现

```text
TWR
XIRR
Realized Gain
Advanced Cost Basis
Benchmark
Automation
```

---

---

# v0.1.4 — Analytics & Performance

# 73. 版本定位

## Theme

> **Know why your wealth changed.**

v0.1.4 是 Nestworth 的分析版本。

从这一版开始，需要明确回答：

> 我到底赚了多少钱？

而不仅仅是：

> 我的资产增加了多少？

---

# 74. 核心能力

```text
Cost Basis

Realized Gain

Unrealized Gain

Dividend

Interest

Fee

Investment Return

TWR

XIRR

Net Worth Attribution

FX Attribution

Cash Flow Analytics
```

---

# 75. Cost Basis

Holding 增加：

```text
costBasis
averageCost
```

内部通过 Activity 重建。

不要允许：

```text
Holding.currentValue - InitialValue
```

这种错误方式估算收益。

---

# 76. 第一阶段成本法

v0.1.4 暂定：

```text
Weighted Average Cost
```

作为默认成本模型。

未来再考虑：

```text
FIFO
LIFO
Specific Lot
```

---

# 77. Unrealized Gain

```text
Current Market Value
-
Remaining Cost Basis
```

---

# 78. Realized Gain

卖出：

```text
Sale Proceeds

-

Allocated Cost Basis

-

Fees

=

Realized Gain
```

---

# 79. Income

单独统计：

```text
Dividend
Interest
Distribution
```

不要混入：

```text
Capital Gain
```

---

# 80. Investment Performance Page

增加：

```text
Investments
    Overview
    Holdings
    Performance
```

---

# 81. Portfolio Metrics

显示：

```text
Current Value

Total Cost Basis

Unrealized Gain

Realized Gain

Dividend / Interest

Total Gain

Return %
```

---

# 82. TWR

实现：

```text
Time Weighted Return
```

用于：

- Portfolio
- Account
- Instrument

主要用于排除外部现金流影响。

---

# 83. XIRR

实现：

```text
Money Weighted Return
```

适合：

- 定投
- 长期 Portfolio
- 房地产
- 私人投资

---

# 84. 时间范围

统一：

```text
1M
3M
6M
YTD
1Y
3Y
5Y
ALL
Custom
```

---

# 85. Net Worth Change Attribution

例如：

```text
Last 12 Months

Net Worth Change

+¥350,000
```

拆解：

```text
External Cash Flow

+¥220,000


Investment Gain

+¥90,000


FX Effect

+¥20,000


Property Revaluation

+¥30,000


Fees

-¥10,000
```

---

# 86. External vs Internal Flow

必须从 Activity 中明确判断：

```text
External Flow
```

例如：

```text
Salary
Inheritance
External Deposit
Expense
```

和：

```text
Internal Flow
```

例如：

```text
DBS → MooMoo
MooMoo Cash → QQQ
```

内部移动不能算 Net Worth Growth。

---

# 87. FX Attribution

例如 QQQ：

```text
QQQ Return

Security:
+12%

FX:
+3%

Combined:
+15.36%
```

至少提供金额层面的 FX Contribution。

---

# 88. Cash Flow

虽然 Nestworth 不是记账 App，但可以提供高层级 Cash Flow：

```text
External Inflow

External Outflow

Net External Flow
```

例如：

```text
Salary
Dividend
Investment Contribution
Large Expense
Debt Repayment
```

不需要提供：

```text
餐饮分类
交通分类
娱乐预算
```

---

# 89. Analytics Dashboard

新增：

```text
Analytics
```

页面包含：

### Net Worth

```text
Trend
Change
Attribution
```

### Investment

```text
Return
Gain
Income
```

### Allocation

```text
Category
Instrument
Currency
Country
Member
Institution
Group
```

### Cash Flow

```text
External Inflow
External Outflow
```

---

# 90. Holding Detail

例如：

```text
QQQ

Market Value
¥144,900

Quantity
30

Average Cost
$520

Price
$700

Unrealized Gain
+¥37,000

Total Return
+31.2%
```

---

# 91. Performance Chart

支持：

```text
Portfolio Value

Net Contributions

Investment Gain
```

避免单纯展示：

```text
Portfolio Value
```

因为用户存入更多资金时，Portfolio Value 增长并不代表投资赚钱。

---

# 92. Benchmark

v0.1.4 可以加入基础 Benchmark 能力：

例如：

```text
QQQ
SPY
ES3
```

用户为 Portfolio 指定 Benchmark。

只实现：

```text
Portfolio TWR
vs
Benchmark Return
```

复杂 Risk Metrics 暂时不做。

---

# 93. v0.1.4 验收标准

必须正确处理：

### Case

用户：

```text
Jan 1
投入 $10,000

Mar 1
投入 $5,000

Jun 1
卖出部分资产

Jul
收到 Dividend

Aug
产生 Fee
```

系统能够分别计算：

```text
Current Value
Cost Basis
Realized Gain
Unrealized Gain
Dividend
Fee
TWR
XIRR
```

且：

```text
Transfer between accounts
```

不得改变 Household Investment Return。

---

# 94. v0.1.4 暂不实现

```text
Tax Reporting
Tax Lot Optimization
Monte Carlo
FIRE Planning
Options
Derivatives
Advanced Risk Metrics
```

---

---

# v0.1.5 — Automation & Productization

# 95. 版本定位

## Theme

> **Make Nestworth sustainable for long-term use.**

前四个版本回答了：

```text
现在有什么
值多少钱
发生了什么
赚了多少
```

v0.1.5 主要解决：

> 怎样让用户几个月、几年持续维护，而不会觉得麻烦？

---

# 96. 核心能力

```text
Automation

Recurring Activities

Pending Activities

Data Freshness

Reminders

Backup

Restore

CSV Import

CSV Export

JSON Backup

Command Palette

Global Search

UX Polish
```

---

# 97. Automation

Sidebar：

```text
Automation
```

支持创建：

```text
Recurring Income

Recurring Transfer

Recurring Investment

Recurring Debt Payment

Recurring Manual Adjustment

Valuation Reminder
```

---

# 98. Salary Automation

例如：

```text
Salary

Every month
25th

DBS Multiplier

+SGD 8,000

Type:
Income
```

---

# 99. Recurring Transfer

例如：

```text
DBS

→

MooMoo SG

SGD 2,000

Every month
28th
```

---

# 100. Recurring Investment

例如：

```text
Buy

QQQ

Every month

Quantity:
1
```

Investment Automation 默认生成：

```text
Pending Activity
```

而不是直接 Confirm。

因为真实：

```text
Price
FX
Fee
Execution Date
```

可能和计划不同。

---

# 101. Pending Activity

Automation 产生：

```text
Pending
```

用户打开：

```text
Confirm
Edit
Skip
```

例如：

```text
Scheduled

Buy 1 QQQ

Expected:
$700

Actual:
$703.21

Fee:
$1.99
```

确认后才进入正式 Ledger。

---

# 102. Automation Schema

```text
automation_rules
automation_runs
```

Rule：

```text
type

schedule

sourceAccount
destinationAccount

instrument

amount
quantity

currency

nextRunAt

enabled
```

---

# 103. Data Freshness

Dashboard 增加：

```text
Data Health
```

例如：

```text
96% Fresh
```

详细：

```text
DBS
Updated today

MooMoo
Updated today

Apartment
43 days ago

Car
128 days ago
```

---

# 104. Asset Refresh Policy

Account 可以设置：

```text
Refresh Frequency
```

例如：

```text
Daily
Weekly
Monthly
Quarterly
Yearly
Never
```

主要用于：

```text
房产
汽车
应收账款
私人投资
```

---

# 105. Reminder

例如：

```text
Apartment valuation hasn't been updated
for 90 days.
```

这里只生成：

```text
App 内提醒
```

后续再考虑 macOS Notification。

---

# 106. Backup

必须支持：

```text
Full Backup
```

包含：

```text
Database
Images
Settings
Metadata
```

导出格式可以：

```text
.nestworth
```

内部：

```text
ZIP
```

例如：

```text
database.sqlite
manifest.json
assets/
```

---

# 107. Automatic Local Backup

支持：

```text
Daily
Weekly
Manual
```

保留：

```text
最近 N 个版本
```

例如默认：

```text
10
```

---

# 108. JSON Export

提供完整：

```text
Machine-readable Export
```

保证用户不会被锁定在 Nestworth。

---

# 109. CSV Export

支持导出：

```text
Accounts

Holdings

Activities

Net Worth Snapshots
```

---

# 110. CSV Import

第一版支持：

## Account Import

例如：

```text
name
institution
category
currency
balance
owner
```

## Holding Import

例如：

```text
account
symbol
quantity
cost
currency
```

## Activity Import

提供 Nestworth Standard CSV。

---

# 111. Broker Import

v0.1.5 只建立：

```text
Importer Interface
```

例如 Rust：

```text
Importer
```

不同 Broker：

```text
MooMoo
IBKR
Tiger
```

以后可以作为独立 Importer 添加。

v0.1.5 不要求一次支持大量券商。

---

# 112. Global Search

支持：

```text
⌘F
```

搜索：

```text
Account
Member
Institution
Group
Instrument
```

---

# 113. Command Palette

支持：

```text
⌘K
```

例如：

```text
Add Account

Update Balance

Transfer

Buy

Sell

Refresh Prices

Refresh FX

Add Member

Open MooMoo SG

Search QQQ
```

---

# 114. Keyboard First

macOS 重点支持：

```text
⌘N
New

⌘K
Command Palette

⌘F
Search

⌘R
Refresh

⌘,
Settings
```

---

# 115. Account Quick Actions

右键：

```text
Update Balance

Transfer

Add Holding

Buy

Sell

Edit

Archive
```

---

# 116. Empty State

第一次启动不要显示复杂 Dashboard。

显示：

```text
Welcome to Nestworth

Build a clear picture of your household wealth.

[ Create Household ]
```

之后：

```text
Add your first account

Bank Account
Investment
Property
Liability
Other
```

---

# 117. Onboarding

推荐：

### Step 1

```text
Create Household
```

### Step 2

```text
Choose Base Currency
```

### Step 3

```text
Add Members
```

### Step 4

```text
Add First Account
```

不要强迫用户配置所有设置。

---

# 118. v0.1.5 验收标准

完成：

```text
工资自动记录

每月转账

定投 Pending Activity

定期资产估值提醒
```

用户能够：

```text
Export Backup

Restore Backup

Export CSV

Import Accounts

Import Holdings
```

完整数据在：

```text
No Internet
```

环境下仍可以浏览和编辑。

---

# 119. v0.1.5 完成后的产品能力

此时 Nestworth 已经具备：

## Household

```text
Household
Member
Joint Ownership
```

## Asset Management

```text
Institution
Account
Group
Category
```

## Portfolio

```text
Stock
ETF
Fund
Crypto
Bond
Other Investment
```

## Multi Currency

```text
FX
Price
Quote Cache
Batch Refresh
```

## Ledger

```text
Deposit
Withdrawal
Transfer
Buy
Sell
Dividend
Interest
Fee
Debt
Adjustment
```

## History

```text
Net Worth History
Account History
Portfolio History
```

## Analytics

```text
Assets
Liabilities
Net Worth
Allocation
Cash Flow
Cost Basis
Gain / Loss
TWR
XIRR
FX Attribution
```

## Automation

```text
Salary
Transfer
Investment
Debt
Valuation Reminder
```

## Data Ownership

```text
SQLite
Local First
Backup
Restore
CSV
JSON
```

---

# 120. 各版本功能矩阵

| Feature | 0.1.1 | 0.1.2 | 0.1.3 | 0.1.4 | 0.1.5 |
|---|---:|---:|---:|---:|---:|
| Household | ✓ | ✓ | ✓ | ✓ | ✓ |
| Member | ✓ | ✓ | ✓ | ✓ | ✓ |
| Ownership | ✓ | ✓ | ✓ | ✓ | ✓ |
| Institution | ✓ | ✓ | ✓ | ✓ | ✓ |
| Group | ✓ | ✓ | ✓ | ✓ | ✓ |
| Account | ✓ | ✓ | ✓ | ✓ | ✓ |
| Assets / Liabilities | ✓ | ✓ | ✓ | ✓ | ✓ |
| Net Worth | ✓ | ✓ | ✓ | ✓ | ✓ |
| i18n | ✓ | ✓ | ✓ | ✓ | ✓ |
| Multi Currency |  | ✓ | ✓ | ✓ | ✓ |
| FX |  | ✓ | ✓ | ✓ | ✓ |
| Instrument |  | ✓ | ✓ | ✓ | ✓ |
| Holding |  | ✓ | ✓ | ✓ | ✓ |
| Stock Price API |  | ✓ | ✓ | ✓ | ✓ |
| Batch Refresh |  | ✓ | ✓ | ✓ | ✓ |
| Transfer |  |  | ✓ | ✓ | ✓ |
| Buy / Sell |  |  | ✓ | ✓ | ✓ |
| Dividend |  |  | ✓ | ✓ | ✓ |
| Fee |  |  | ✓ | ✓ | ✓ |
| Activity Ledger |  |  | ✓ | ✓ | ✓ |
| History |  |  | ✓ | ✓ | ✓ |
| Net Worth Chart |  |  | ✓ | ✓ | ✓ |
| Cost Basis |  |  | △ | ✓ | ✓ |
| Realized Gain |  |  |  | ✓ | ✓ |
| Unrealized Gain |  |  |  | ✓ | ✓ |
| TWR |  |  |  | ✓ | ✓ |
| XIRR |  |  |  | ✓ | ✓ |
| Attribution |  |  |  | ✓ | ✓ |
| Automation |  |  |  |  | ✓ |
| Data Freshness |  | △ | △ | △ | ✓ |
| CSV Import |  |  |  |  | ✓ |
| CSV Export |  |  |  |  | ✓ |
| Full Backup |  |  |  |  | ✓ |
| Command Palette |  |  |  |  | ✓ |

`△` 表示已有基础数据或内部能力，但还不是完整用户功能。

---

# 121. 推荐开发顺序

每一个版本内部都建议遵循：

```text
Schema

↓

Domain Model

↓

Repository

↓

Service

↓

Tauri Commands

↓

TypeScript API Client

↓

UI

↓

Tests
```

不要反过来先写页面，再临时设计 SQLite Schema。

---

# 122. Rust 模块建议

最终可以形成：

```text
src-tauri/src/

domain/
    household.rs
    member.rs
    institution.rs
    account.rs
    ownership.rs
    group.rs
    instrument.rs
    holding.rs
    activity.rs
    quote.rs
    money.rs

application/
    household_service.rs
    account_service.rs
    portfolio_service.rs
    valuation_service.rs
    activity_service.rs
    analytics_service.rs
    automation_service.rs

infrastructure/
    database/
    repositories/
    quote_providers/
    fx_providers/
    importers/
    backup/

commands/
    household.rs
    account.rs
    portfolio.rs
    activity.rs
    analytics.rs
    automation.rs
```

---

# 123. TypeScript 模块建议

```text
src/

features/
    overview/
    accounts/
    members/
    institutions/
    groups/
    investments/
    activity/
    analytics/
    automation/
    settings/

components/

lib/
    tauri/
    money/
    charts/
    i18n/

types/
```

---

# 124. IPC 设计

不要暴露几十个细粒度数据库 Command，例如：

```text
insert_account
select_account
update_account_row
delete_account_row
```

而暴露业务接口：

```text
create_account()

update_account()

update_balance()

transfer()

buy_instrument()

sell_instrument()

refresh_quotes()

get_net_worth_summary()

get_portfolio_performance()
```

也就是：

> Tauri IPC 应表达 Domain Action，而不是 SQLite CRUD。

---

# 125. 数据库 Migration

从第一个版本开始：

```text
001_initial.sql

002_portfolio.sql

003_activity.sql

004_analytics.sql

005_automation.sql
```

每个 Nestworth Release 都必须支持：

```text
old database
↓
automatic migration
↓
new database
```

Migration 必须进入自动测试。

---

# 126. 测试优先级

Nestworth 最值得重点测试的不是 UI，而是金融计算。

## P0

```text
Net Worth

Ownership

FX Conversion

Holding Value

Internal Transfer

Credit Card Repayment

Buy / Sell

Cost Basis

TWR

XIRR
```

---

# 127. Golden Test Cases

建议维护一套固定 Household Fixtures：

```text
Wang Family

Walt
Spouse

DBS
MooMoo SG
WeChat

Home
Mortgage

QQQ
ES3
```

每次修改 Calculation Engine 后执行相同的 Golden Tests。

例如预期：

```text
Assets:
¥4,800,000

Liabilities:
¥1,000,000

Net Worth:
¥3,800,000
```

避免后期某次改汇率逻辑导致历史结果全部改变。

---

# 128. v0.1.5 后暂时不要急于开发的能力

完成 MVP 后，建议先让真实用户维护几个月，而不是立即增加：

```text
Bank Sync

Plaid

Open Banking

Broker Direct API

Crypto Wallet Sync

Cloud Account System

Family Remote Collaboration

AI Financial Advisor

Budgeting

Expense Categorization

Tax Reporting
```

这些功能任何一个都可能显著改变项目复杂度。

---

# 129. v0.2.x 可以考虑的方向

如果 v0.1.5 使用体验稳定，再开始考虑：

## v0.2 Portfolio

```text
Tax Lots
Advanced Benchmark
Portfolio Rebalancing
Target Allocation
Dividend Forecast
```

## v0.2 Planning

```text
Financial Goals
Retirement
FIRE
Forecast
Scenario Planning
```

## v0.2 Integration

```text
Broker CSV Adapter
Broker API
Bank API
Crypto Wallet
```

## v0.2 Sync

```text
iCloud
Multi-device
Household Collaboration
```

---

# 130. 最重要的五个里程碑

整个 `v0.1.x` 实际上可以浓缩为五个问题。

### v0.1.1

> **What do we own and owe?**

### v0.1.2

> **What is it worth right now?**

### v0.1.3

> **What changed?**

### v0.1.4

> **Why did our wealth change, and how did our investments perform?**

### v0.1.5

> **How can Nestworth keep this information accurate with minimal maintenance?**

当这五个问题都能很好地回答时，Nestworth 的第一阶段产品就已经成立。

---

# 131. 最终 MVP 定义

Nestworth v0.1.5 的核心体验应该是：

```text
Open Nestworth

↓

¥3,842,392
Net Worth

↓

See

Assets
Liabilities
Investments
Cash

↓

Understand

Where the money is
Who owns it
How it changed
What generated returns

↓

Maintain

Update
Refresh
Transfer
Import
Automate
```

这应该是整个 `v0.1.x` 产品线所有设计决策的判断标准：

> **帮助用户用尽可能低的维护成本，长期保持一张准确、可解释的家庭资产负债表。**