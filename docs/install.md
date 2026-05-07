# Install

> Pick one. They're the same binary.

> Pre-release: install paths go through GitHub. `pip install beava`,
> `brew install beava-dev/tap/beava`, and the prebuilt Docker image will
> light up at the v0.0.0 cut.

## pip (from GitHub)

```bash
pip install "git+https://github.com/beava-dev/beava.git#subdirectory=python"
```

The Python SDK ships with the server binary embedded. `bv.App()` discovers and runs it on an ephemeral port. This is what you want for development, tests, and most production deployments.

## Docker (build from source)

```bash
git clone https://github.com/beava-dev/beava
cd beava
docker build -f deploy/Dockerfile.beava -t beava:dev .
docker run -p 8080:8080 -v $PWD/beava.example.yaml:/app/beava.yaml beava:dev
```

Push and query against `:8080`. Mount a volume at `/data` to persist the WAL and snapshots.

## Cargo (build from source)

```bash
git clone https://github.com/beava-dev/beava
cd beava
cargo build --release -p beava-server
./target/release/beava -c beava.example.yaml
```

The binary takes a single YAML config (default `./beava.yaml`); ports come
from `listen_addr` / `admin_addr` keys, or env vars
`BEAVA_LISTEN_ADDR` / `BEAVA_ADMIN_ADDR`. There is no `serve` subcommand.

## Verify

```bash
$ beava --version
beava 0.0.0
```

Or from Python:

```python
import beava as bv
print(bv.__version__)
```

## What's next

[Quickstart](/docs/get-started/quickstart/) — first feature in 60 seconds.
