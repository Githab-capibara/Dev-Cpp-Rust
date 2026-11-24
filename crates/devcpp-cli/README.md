# devcpp-cli

Командная строка поверх [`devcpp-core`](../devcpp-core/README.md): те же
проекты, та же сборка, тот же отладчик — без GUI. Удобно для CI, скриптов и
проверки состояния машины.

| Команда | Что делает |
|---------|------------|
| `devcpp new <name> [--template T] [--dir D]` | Создать проект из шаблона; печатает путь к `.dev` |
| `devcpp build <file.dev>` | Собрать проект (флаг `--clean` — с нуля) |
| `devcpp run <file.dev>` | Собрать и запустить (`RunMode::BuildAndRun`) |
| `devcpp debug <file.dev>` | Собрать и запустить под GDB |
| `devcpp info <file.dev>` | Информация о проекте: юниты, опции, версия |
| `devcpp tools` | Доступные инструменты: компиляторы, make, gdb |
| `devcpp templates` | Шаблоны проектов с описаниями |

Код возврата — статус сборки: сломанный проект даёт `1`, поэтому `devcpp`
можно ставить в CI. Каждая диагностика GCC печатается один раз: дубль из
`BuildMessage::Diagnostic` отключён намеренно, сырая строка компилятора уже
содержит файл, строку и сообщение.

## Пример

```bash
./target/release/devcpp templates                    # что можно создать
./target/release/devcpp tools                        # готовность машины к сборке
./target/release/devcpp new hello --dir ./projects   # создать проект
./target/release/devcpp run ./projects/hello.dev     # собрать и выполнить
```

## Тесты

```bash
cargo test -p devcpp-cli   # 5 e2e: new/build/run на реальном g++, exit codes
```