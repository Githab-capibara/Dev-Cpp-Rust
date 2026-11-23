# 02. Держим мозг IDE в GUI-free crate devcpp-core

- **Status:** Accepted
- **Date:** 2026-11-23
- **Deciders:** @Githab-capibara
- **Related:** [01. GUI-стек](01-gui-stack-gtk4-sourceview.md)

## Context

Вся логика IDE — чтение/запись `.dev`, генерация Makefile, парсинг вывода
компилятора, диалог с GDB, индекс символов — не требует экрана. Если она живёт
в GUI-коде, её можно проверить только с display server (Xvfb), что делает
тесты медленными и хрупкими.

## Decision

**Разделяем на `devcpp-core` (чистая библиотека, ноль GTK) и `devcpp-gtk`
(тонкий визуальный слой).** CLI (`devcpp-cli`) тоже строится только на core.
Граница: core экспортирует типы (`Project`, `BuildJob`, `Debugger`,
`SymbolIndex`, `Template`, `Config`), GUI лишь отображает их состояние и
пересылает события; каналы crossbeam + `glib::timeout_add_local` для поллинга.

## Consequences

- **Easier:** 73 юнит-теста + 5 e2e идут без X/ Wayland за ~2 с; логику можно
  переиспользовать из CLI.
- **Harder:** нужен аккуратный контракт «сообщение → UI-обновление»; часть
  поведений (автопоказ комплишена по набранной точке) остаётся в GUI.
- **Given up:** ничего.
- **Migration:** n/a.

## Alternatives considered

- **Всё в одном крейте с `#[cfg(test)]` + Xvfb:** отвергнуто — e2e на GUI
  требует Xvfb в CI, ловит гонки и остаётся недетерминированным.
- **Компоненты через trait-абстракцию UI:** избыточно для трёх крейтов.