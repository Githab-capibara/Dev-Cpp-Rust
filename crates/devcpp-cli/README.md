# devcpp-cli

Командная строка поверх [`devcpp-core`](../devcpp-core/README.md): те же
проекты, та же сборка, тот же отладчик — без GUI. Удобно для CI, скриптов и
проверки состояния машины.

| Команда | Что делает |
|---------|------------|
| `devcpp build <file.dev>` | Собрать проект (флаг `--rebuild` — с нуля) |
| `devcpp run <file.dev>` | Собрать и запустить (`RunMode::BuildAndRun`) |
| `devcpp debug <file.dev>` | Собрать и запустить под GDB |
| `devcpp info <file.dev>` | Информация о проекте: юниты, опции, версия |
| `devcpp tools` | Доступные инструменты: компиляторы, make, gdb |
| `devcpp templates` | Шаблоны проектов с описаниями |

## Пример

```bash
./target/release/devcpp templates          # что можно создать
./target/release/devcpp tools              # готовность машины к сборке
./target/release/devcpp run hello.dev      # собрать и выполнить
```