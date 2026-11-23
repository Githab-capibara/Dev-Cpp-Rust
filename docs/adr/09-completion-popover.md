# 09. Комплишн — поповер GTK4 поверх headless-движка

- **Status:** Accepted
- **Date:** 2026-11-24
- **Deciders:** @Githab-capibara
- **Related:** [02. Core без GUI](02-core-without-gui.md)

## Context

`GtkSourceCompletionWords` принимает фиксированный список слов в момент
создания, а IDE нужен живой индекс символов проекта, который меняется при
открытии файлов. Dev-C++ показывает список членов после `.`/`->` и по
`Ctrl+Space`, вставляет по Tab/Enter, закрывает по Escape.

## Decision

**Свой поповер:** `devcpp-core::completion` считает кандидатов по позиции
курсора (префикс + контекст — точка/стрелка/область), GUI рисует
`gtk::Popover` со списком. Движок остаётся GUI-free; автопоп-ап ставится по `.`/`->` через контроллер клавиш с проверкой в idle (курсор мог уехать),
`Ctrl+Space` — через акселератор `win.complete`.

## Consequences

- **Easier:** живой индекс проекта; один источник кандидатов для CLI/GUI;
  поведение совпадает с Dev-C++.
- **Harder:** свой рендер списка (без «шорткат-превью» GtkSourceView).
- **Given up:** ничего.
- **Migration:** n/a.

## Alternatives considered

- **GtkSourceCompletionProvider:** отвергнуто — API провайдеров требует
  пересобирать `GtkSourceCompletionWords` под каждый индекс и тянет
  лишний слой.