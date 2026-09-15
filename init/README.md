# Initialization for EDMS application

This folder is the single entry point for running EDMS — clone the repo,
follow the two steps below, and you get a working app with sample data
already loaded.

## 1. Configure your storage path (required)

EDMS keeps all its data — collections, request/response history, everything
under `storage/` — in a directory on your own machine, not inside this repo.

```bash
cd init
cp .env.example .env
```

Edit `.env` and set `EDMS_HOST_PATH` to a real directory **outside this
repo**, then **create that directory yourself** (the app will not create
it for you):

```bash
mkdir -p ../../edms-data   # example — anywhere outside the repo works
```

```
# init/.env
EDMS_HOST_PATH=../../edms-data
```

If you skip this, `docker compose up` will refuse to start and tell you
exactly what to do.

## 2. Run it

```bash
docker compose up --build
```

First run takes a few minutes (Rust release build). Once it's up:

- **App:** http://localhost:3911
- **Backend API:** http://localhost:3000 (see `backend/webserver/API_REFERENCE.md`)

A one-shot `seed` container runs automatically the first time and
populates ~25 real, tested endpoints with history, collections, and tags,
so the app isn't empty on first look. It needs outbound internet (it
tests against a public API) and only runs once — safe to leave in place
on every `docker compose up`.

## Resetting

```bash
docker compose down -v
rm -rf ../backend/webserver/data
```

Your `EDMS_HOST_PATH` directory (outside the repo) is untouched by this —
delete it yourself if you want a truly clean slate.
