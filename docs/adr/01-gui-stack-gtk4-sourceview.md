# 01. Выбираем GTK4 + GtkSourceView 5 как GUI-стек

- **Status:** Accepted
- **Date:** 2026-11-23
- **Deciders:** @Githab-capibara
- **Related:** [02. Core без GUI](02-core-without-gui.md)

## Context

Нужен клон Dev-C++ (Windows/GTK-приложение) под Linux с мгновенным запуском.
Кандидаты: Qt6 (QScintilla), GTK4 (GtkSourceView), Electron (отпал сразу —
отрицательная производительность и не-нативность), Iced/egui (слишком сырые
для полноценного IDE).

Ключевые требования: нативный вид в GNOME/Hyprland, mature-виджет редактора с
подсветкой синтаксиса, мгновенный старт, Rust-привязки первого класса.

## Decision

**GTK4 + GtkSourceView 5.** Реализуем через `gtk4-rs` + `sourceview5-rs`.
MDI-модель Dev-C++ заменяется на notebook-вкладки (в Linux MDI не уживается).
Дерево проекта остаётся `TreeView`: штатная альтернатива `ColumnView` в GTK 4.10
не моделирует дерево; deprecation подавляется осознанно crate-wide.

## Consequences

- **Easier:** нативный запуск, единый тулкит с окружением пользователя,
  готовый редактор с гаттерами; `devcpp` стартует за ~50 мс.
- **Harder:** менюбар живёт в header bar через `PopoverMenuBar` (GTK4 убрал
  menu-модель с окон); часть API GTK3 (FileChooserButton) отсутствует —
  заменяем на `FileChooserNative`.
- **Given up:** pixel-perfect копия панелей Windows Dev-C++; MDI вместо
  вкладок.
- **Migration:** n/a.

## Alternatives considered

- **Qt6/QScintilla:** мощнее, но тянет ~30 МБ Qt-рантайма и рендерит не в
  стиле окружения; rust bindings (`krustning`) через C++-мост — тяжелее
  pure-Rust разработки.
- **Electron:** отвергнут по требованию «мгновенно и нативно».
- **Iced/egui:** нет зрелого редактора исходников с подсветкой C/C++.