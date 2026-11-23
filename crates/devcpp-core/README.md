# devcpp-core

Мозг IDE без GUI: чистый Rust, ноль зависимостей от GTK, полностью тестируем
без display server. CLI и GTK-фронтенд — тонкие обёртки над этим крейтом.

| Модуль | Что делает | Ключевой API |
|--------|------------|--------------|
| [`project`](src/project.rs) | Формат `.dev` Dev-C++ 6.x: парсинг, сохранение, round-trip | `Project::load` / `::save`, `ProjectType`, `Unit` |
| [`makefile`](src/makefile.rs) | Генератор Makefile: C/C++ юниты, флаги, `obj/`, `bin/` | `makefile::generate`, `makefile::write`, `compile_flags` |
| [`build`](src/build.rs) | Фоновый джоб сборки; `RunMode::BuildAndRun` стримит вывод программы | `build::start`, `BuildJob`, `BuildMessage`, `RunMode` |
| [`diagnostics`](src/diagnostics.rs) | Парсер диагностик GCC/Clang в кликабельные ошибки | `diagnostics::parse`, `Diagnostic`, `Severity` |
| [`toolchain`](src/toolchain.rs) | Детект компиляторов, отладчика, make | `Toolchain::detect`, `can_build`, `can_debug` |
| [`debugger`](src/debugger.rs) | Отладочные сессии поверх GDB: breakpoints, стек, переменные | `Debugger::start`, `DebugEvent`, `Breakpoint` |
| [`mi`](src/mi.rs) | Парсер GDB MI-2: типизированные records и значения | `mi::parse`, `Record`, `Value::path` |
| [`symbols`](src/symbols.rs) | Индекс символов из tree-sitter C/C++ | `SymbolIndex::build`, `classes`, `members_of` |
| [`completion`](src/completion.rs) | Кандидаты комплишна по позиции курсора | `completion::context_at`, `completion::candidates` |
| [`templates`](src/templates.rs) | Шаблоны проектов Dev-C++ (5 штук) | `templates::create`, `templates::find_by_name` |
| [`tools`](src/tools.rs) | Менеджер внешних инструментов с плейсхолдерами | `ToolManager::run`, `ToolEntry::expand_args` |
| [`config`](src/config.rs) | TOML-конфиг: редактор, сборка, not-so-recent проекты | `Config::load` / `::save`, `push_recent` |
| [`paths`](src/paths.rs) | Манипуляции путями в духе Dev-C++ | `expand_file`, `relative_path`, `change_ext` |

## Пример

```rust
use devcpp_core::{build::{self, RunMode}, project::Project};

let project = Project::load("hello.dev")?;
let job = build::start(&project, RunMode::BuildOnly)?;
while let Ok(msg) = job.recv() {
    // stdout компилятора построчно, затем Finished(bool)
}
```

Живой e2e-пример — [`tests/e2e.rs`](tests/e2e.rs): создать проект из шаблона,
собрать через `make`, запустить, проверить вывод.