# 12. Find/Replace — панель-бар и `sourceview::SearchContext`

- **Status:** Accepted
- **Date:** 2026-11-24
- **Deciders:** @Githab-capibara
- **Related:** [01. GUI-стек](01-gui-stack-gtk4-sourceview.md), [11. Undo/Redo](11-undo-redo-snapshot-stack.md)

## Context

Dev-C++ ищет и заменяет через модальное окно с полями Find/Replace,
галочек Match case/Whole words и кнопками Replace/Replace All. В клоне
нужен тот же результат без модального окна: панель под вкладками редактора
не блокирует набор, как поиск в Zed/VSCode.

## Decision

**Скрытый бар над редактором** (`Editor::install_find_bar`): строка поиска,
кнопки Previous/Next/Close, ниже — строка замены с Replace/Replace All.
Живёт в `Editor`, а не в окне: каждая вкладка получает свой бар, Escape
закрывает его (контроллер клавиш сначала на записях, затем на окне как
fallback), `Return` в поле поиска = следующее совпадение,
`Shift+Return` = предыдущее, `Return` в поле замены = Replace,
`Ctrl+F`/`Ctrl+H` (акселераторы `win.find`/`win.replace`) открывают бар.
Кнопки выведены из tab-цепочки (`set_focusable(false)`), чтобы Tab ходил
Entry → Entry.

Сам поиск — `sourceview::SearchContext` (`ensure_search` создаёт контекст с
`highlight = true`), `forward`/`backward` ищут от курсора и выделяют
совпадение, `replace`/`replace_all` правят буфер. Контекст инвалидируется
(`search.borrow_mut().take()`) при изменении текста поиска, чтобы не искать
старым регэкспом. Найденное количество показывается в лейбле («Line 1»).

## Consequences

- **Easier:** поиск повторяет нативные жесты GtkSourceView (выделение,
  подсветка всех вхождений, wrap-around), не нужно писать свой движок
  обхода; undo после Replace работает тем же snapshot-механизмом (ADR-11).
- **Harder:** `SearchContext` не переживает смену языка/буфера — бар
  пересоздаёт его лениво; ошибка «не нашел» показывается только лейблом,
  без диалога.
- **Given up:** галочек Match case / Whole words пока нет — поиск
  регистронезависимый; расширяется настройками SearchContext.
- **Migration:** n/a.

## Alternatives considered

- **Модальный диалог как в Dev-C++:** отвергнуто — блокирует редактор,
  на Wayland лишний `GtkWindow`; бар-под-вкладками стал стандартом в IDE.
- **Полностью свой поиск по тексту буфера:** отвергнуто — дублирует
  поиск/подсветку GtkSourceView и теряет интеграцию с regions.
- **Поиск в отдельном виджете «Find results»:** отложено — Dev-C++ так не
  делает, а Compiler Output уже занимает нижнюю панель.