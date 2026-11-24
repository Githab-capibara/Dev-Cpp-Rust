# 01. Architecture — core без GUI, GUI-слой, пайплайн сборки

- **Status:** Accepted
- **Date:** 2026-11-23
- **Deciders:** @Githab-capibara
- **Feeds into:** docs/adr/01-12

## Формула

```
devcpp-core   — чистая библиотека, ноль зависимостей от GTK, полностью тестируема
devcpp-cli   — тонкая обёртка над core для терминала
devcpp-gtk   — 100% визуальный слой; любая логика живёт в core
```

## Потоки данных

1. **Создание проекта.** Мастер `New Project` (имя, тип из `templates::TEMPLATES`,
   папка) → `templates::create` пишет `.dev` (INI-формат Dev-C++) и исходники →
   `Project::load` → дерево проекта и класс-браузер заполняются из модели.
2. **Компиляция.** Кнопка/`F9` → `build::start(project, mode)` → фоновый поток:
   `makefile::write` → `make -f Makefile -j N` → stdout/stderr строчно идут в
   `BuildMessage::Line` / `::Diagnostic` → `poll_build` раз в 60 мс сливает их в
   Compiler Output и статус-бар. Диагностики помнят свою строку в логе, поэтому
   клик по ошибке = переход в редактор (`goto_line`).
3. **Compile & Run.** Тот же джоб в режиме `RunMode::BuildAndRun`: после успешной
   сборки поток запускает программу и стримит её stdout/stderr теми же
   `BuildMessage::Line` — Compiler Output показывает компилятор и программу
   одним логом, как в Dev-C++.
4. **Отладчик.** `Debugger::start(gdb)` → GDB MI-2 диалог (`mi.rs`) → события
   `DebugEvent` → панели Call Stack / Locals / Breakpoints / Debug Console.
5. **Комплишн.** `symbols::SymbolIndex` (tree-sitter-c/cpp) + `completion.rs`
   (контекст по строке) → всплывающий список в редакторе; вызывается по
   `Ctrl+Space` и автоматически после `.` или `->`. Триггером служит
   `buffer.changed`, а не клавиатура: на Wayland символы приходят
   text-commit'ом (см. ADR-11).
6. **Undo/Redo и Find/Replace.** `Editor` владеет стеком snapshot'ов буфера
   (батч открывает первое изменение после 500 мс тишины) и Find/Replace-баром
   на `sourceview::SearchContext`; оба — GUI-слой, потому что опираются на
   виджеты GtkSourceView (ADR-11, ADR-12).
7. **CLI-поток.** `devcpp new` → `templates::create` → `.dev` + исходники;
   `devcpp build/run` → `build::start` с тем же потоковым выводом, что и в
   GUI; CLI повторяет пайплайн и проверяется интеграционными тестами на
   реальном компиляторе.
8. **Запуск IDE.** `devcpp-gtk [file.dev | main.cpp]` — `HANDLES_OPEN`:
   `.dev` открывает проект и все его юниты, остальное — вкладку редактора;
   одновременно рождается стартовая untitled-вкладка с фокусом в редактор,
   которая сама закрывается, когда появляются настоящие файлы.

## Инварианты

- GUI-код не содержит бизнес-логики: любой прогон, который можно проверить в
  core, проверяется в core (иначе тест требует display server).
- Все внешние процессы фонные, сообщения — через каналы; GUI-поток никогда не
  блокируется.
- Внешний вид и поведение повторяют Dev-C++ 6.x: те же меню, те же шорткаты
  (`Ctrl+F9`/`Ctrl+F10`/`Ctrl+F8`/`F2` плюс редакторные `Ctrl+Z/Y/F/H/X/C/V/A`),
  тот же «клик по ошибке = переход».
- Практика «отсутствия фокуса» исключена: окно стартует с вкладкой-редактором
  под фокусом, диалоги закрываются по Escape, Enter в поле мастера = Create.