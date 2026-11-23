# Architecture Decision Records

Формат [Michael Nygard](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions):
статус, дата, контекст, решение, последствия, альтернативы. Каждая запись —
необратимое на момент принятия решение; отменённые помечаются
`Superseded by ADR-NN`.

| ADR | Решение | Статус |
|-----|---------|--------|
| [01. GUI-стек: GTK4 + GtkSourceView 5](01-gui-stack-gtk4-sourceview.md) | Нативный GTK4-интерфейс, notebook вместо MDI, TreeView для дерева проекта | Accepted |
| [02. Мозг IDE в GUI-free crate devcpp-core](02-core-without-gui.md) | Вся логика тестируется без display server; GUI — тонкий слой | Accepted |
| [03. Формат `.dev` по первоисточнику](03-dev-file-format-from-source.md) | Посимвольная совместимость с Dev-C++ 6.x, round-trip тесты | Accepted |
| [04. Сборка через Makefile и `make` в фоновом джобе](04-makefile-pipeline.md) | Инкрементальность и лог как в оригинале | Accepted |
| [05. GDB MI-2 конечным автоматом](05-gdb-mi-state-machine.md) | Типизированные события вместо regex по строкам | Accepted |
| [06. Гейт качества](06-quality-gates.md) | fmt + clippy `-D warnings` + e2e на реальном компиляторе | Accepted |
| [07. Исходники Dev-C++ (Pascal) вне репозитория](07-reference-sources.md) | `reference/` локальный; атрибуция в README | Accepted |
| [08. Compile & Run: вывод программы через тот же пайплайн](08-build-and-run-streaming.md) | `RunMode::BuildAndRun` исполняется внутри build-джоба | Accepted |
| [09. Комплишн: поповер GTK4 поверх headless-движка](09-completion-popover.md) | Живой индекс символов + `Ctrl+Space`/авто-`.`/`->` | Accepted |
| [10. Лицензия: MIT OR Apache-2.0](10-licensing-mit-apache.md) | Двойная лицензия, оба текста в дереве | Accepted |