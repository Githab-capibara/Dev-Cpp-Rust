# 01. Architecture — core без GUI, GUI-слой, пайплайн сборки

- **Status:** Accepted
- **Date:** 2026-11-23
- **Deciders:** @Githab-capibara
- **Feeds into:** docs/adr/01-10

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
   `Ctrl+Space` и автоматически после `.` или `->`.

## Инварианты

- GUI-код не содержит бизнес-логики: любой прогон, который можно проверить в
  core, проверяется в core (иначе тест требует display server).
- Все внешние процессы фонные, сообщения — через каналы; GUI-поток никогда не
  блокируется.
- Внешний вид и поведение повторяют Dev-C++ 6.x: те же меню, те же шорткаты
  (`Ctrl+F9`/`Ctrl+F10`/`Ctrl+F8`/`F2`), тот же «клик по ошибке = переход».