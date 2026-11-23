# 05. Разбираем GDB MI-2 полноценным конечным автоматом

- **Status:** Accepted
- **Date:** 2026-11-23
- **Deciders:** @Githab-capibara
- **Related:** [02. Core без GUI](02-core-without-gui.md)

## Context

Первая версия отладчика искала в выводе GDB строки-триггеры (`^done`,
`=breakpoint-created`) и зависала, если порядок ответов отличался: асинхронные
события (`*stopped`), stream-логи (`~"..."`, `@"..."`) и result-записи (`^done`)
приходят вперемешку, а `&"..."` (log) — вообще без гарантий порядка.
Тест на breakpoint поймал это: события приходили в «неправильном» порядке, и
дебаг-сессия висла.

## Decision

**Реализуем MI как конечный автомат (`mi.rs`):** типизированные события
(out-of-band records, result records, stream records), очередь команд с
сопоставлением token → ответ, событийная модель `DebugEvent` (Stack,
Variables, Breakpoints, Console, Exited) для UI. Слой отладчика (`debugger.rs`)
говорит по MI-командам (`-break-insert`, `-exec-next`, …) и не зависит от
порядка строк.

## Consequences

- **Easier:** корректная работа с любым порядком ответов; лог GDB виден в
  Debug Console; UI просто подписан на `DebugEvent`.
- **Harder:** нужна дисциплина «команда → token → ответ»; часть команд
  требует синхронного ожидания result (тим-аут в тестах).
- **Given up:** ничего.
- **Migration:** n/a.

## Alternatives considered

- **Regex по строкам вывода:** отвергнуто — хрупко (это и было багом).
- **lldb-mi или DAP (Debug Adapter Protocol):** перспективнее на будущее, но
  Dev-C++ сам работает с GDB — клон остаётся на MI-2.