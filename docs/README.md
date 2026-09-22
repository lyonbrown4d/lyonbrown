# Lyonbrown mdBook 文档

项目文档使用两个结构一致的 mdBook，分别维护中文和英文内容。

## 目录

```text
docs/
├── README.md
├── zh/
│   ├── book.toml
│   └── src/
│       ├── SUMMARY.md
│       ├── architecture/
│       └── plans/
└── en/
    ├── book.toml
    └── src/
        ├── SUMMARY.md
        ├── architecture/
        └── plans/
```

中文是设计与计划的权威版本。涉及含义的修改应先更新中文，再同步英文；两种语言必须保持相同章节结构和链接关系。

## 本地构建

需要 [mdBook](https://rust-lang.github.io/mdBook/) 0.5 或更高版本：

```powershell
mdbook build docs/zh
mdbook build docs/en
```

输出目录：

- `target/mdbook/zh/`
- `target/mdbook/en/`

本地预览单一语言：

```powershell
mdbook serve docs/zh --open
mdbook serve docs/en --open
```

## 编写约定

- 新页面必须加入对应语言的 `src/SUMMARY.md`。
- 内部链接使用相对路径，并在两种语言中保持同构。
- 构建产物只写入 `target/mdbook/`，不得提交生成的 HTML。
- 里程碑状态以中文书的 `src/plans/mvp-implementation-plan.md` 为准。
