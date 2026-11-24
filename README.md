# devcpp — клон Dev-C++ на Rust под Linux

<div align="center">

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)
[![GTK4](https://img.shields.io/badge/GUI-GTK4%20%2B%20GtkSourceView%205-brightgreen.svg)](https://gtk.org)
[![Tests](https://img.shields.io/badge/tests-83%20green-success.svg)](#проверки)

</div>

**devcpp** — нативная среда разработки C/C++ для Linux, повторяющая
Embarcadero Dev-C++ 6.x: мгновенный запуск, нативный GTK4-интерфейс, реальная
компиляция через `make` и полноценный отладчик на GDB MI-2. Без Electron, без
веба, без прослойки виртуальной машины.

## Что внутри

| Компонент | Что делает |
|-----------|------------|
| `devcpp-core` | Мозг IDE без GUI: формат `.dev`, генератор Makefile, авто-детект тулчейна, парсер диагностик GCC/Clang, отладчик GDB MI-2, индекс символов (tree-sitter), комплишн, шаблоны проектов, Tool Manager, TOML-конфиг |
| `devcpp-cli` | Командная строка: создание проектов из шаблонов (`new`), сборка/запуск/отладка, информация о проекте и инструментах |
| `devcpp-gtk` | Интерфейс GTK4 + GtkSourceView 5: меню, тулбар, дерево проекта, класс-браузер, редактор с вкладками (undo/redo, Find/Replace, комплишн), Compiler Output, Debug-панель, мастер «New Project», переход к ошибке кликом, открытие проекта/файла из командной строки |

Ключевая идея архитектуры — см. [docs/design/01-architecture.md](docs/design/01-architecture.md).

## Быстрый старт

```bash
# Сборка (потребует gtk4, gtksourceview-5, gcc/g++, gdb, make)
cargo build --release

# Создать проект из шаблона и посмотреть его
./target/release/devcpp templates
./target/release/devcpp new hello --template "Console Application" --dir ./projects

# Собрать и запустить (CLI повторяет пайплайн IDE)
./target/release/devcpp run ./projects/hello.dev

# Запустить IDE — с проектом или без
./target/release/devcpp-gtk
./target/release/devcpp-gtk ./projects/hello.dev
```

Минимальный пример из кода — в e2e-тестах: они создают проект, собирают его
через `make`, запускают и проверяют вывод (`crates/devcpp-core/tests/e2e.rs`).
CLI покрыт интеграционными тестами на реальном компиляторе
(`crates/devcpp-cli/tests/cli.rs`: `new` → `build` → `run`, ошибка сборки =
ненулевой код, диагностика печатается один раз).

## Горячие клавиши редактора и IDE

| Комбинация | Действие |
|------------|----------|
| `Ctrl+F9` / `Ctrl+F10` / `Ctrl+F8` | Компиляция / Компиляция и запуск / Отладка |
| `F2` | Переключить брейкпоинт |
| `Ctrl+N` / `Ctrl+O` / `Ctrl+S` / `Shift+Ctrl+S` | Новый файл / Открыть / Сохранить / Сохранить как |
| `Ctrl+Q` | Выход |
| `Ctrl+Space` | Автодополнение |
| `Ctrl+Z` / `Ctrl+Y` (`Shift+Ctrl+Z`) | Отменить / Вернуть |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` / `Ctrl+A` | Вырезать / Копировать / Вставить / Выделить всё |
| `Ctrl+F` / `Ctrl+H` | Найти / Заменить (Escape закрывает панель, `Return` — следующее совпадение) |
| `Alt+F`…`Alt+H` | Меню по мнемоникам |

## Проверки

```bash
cargo fmt --all -- --check                     # форматирование
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                          # 83 теста (73 unit + 5 e2e + 5 CLI)
```

## Документация

| Документ | О чём |
|----------|-------|
| [docs/design/01-architecture.md](docs/design/01-architecture.md) | Архитектура: core без GUI, GUI-слой, пайплайн сборки |
| [docs/adr/](docs/adr/README.md) | Архитектурные решения в формате Michael Nygard (01–12) |
| [crates/devcpp-core/README.md](crates/devcpp-core/README.md) | API ядра: проекты, сборка, диагностики, отладчик, символы |
| [crates/devcpp-gtk/README.md](crates/devcpp-gtk/README.md) | GUI-слой: окно, редактор, панели, мастер проектов |
| [crates/devcpp-cli/README.md](crates/devcpp-cli/README.md) | CLI: создание проектов, состояние инструментов |

## Лицензия

Двойная лицензия [MIT](LICENSE-MIT) **OR** [Apache-2.0](LICENSE-APACHE) —
выбирайте любую. Обоснование — [docs/adr/10](docs/adr/10-licensing-mit-apache.md).

## Источник совместимости

Формат `.dev`, шаблоны и генератор Makefile сверены с оригинальными
исходниками Dev-C++ (`reference/*.pas` — Delphi/Pascal). Они **не** входят в
репозиторий, так как являются GPL-кодом upstream-проекта; см.
[docs/adr/07-reference-sources.md](docs/adr/07-reference-sources.md).