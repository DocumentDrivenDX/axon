//! `axon init <project-name>` — scaffold a new Axon project directory.

use anyhow::Result;
use std::path::Path;

// ── Template constants ────────────────────────────────────────────────────────

const TASKS_SCHEMA: &str = r#"{
  "name": "tasks",
  "description": "A simple task tracker",
  "schema": {
    "type": "object",
    "required": ["title", "status"],
    "properties": {
      "title": { "type": "string" },
      "description": { "type": "string", "default": "" },
      "status": { "type": "string", "enum": ["todo", "in_progress", "done"], "default": "todo" },
      "assignee": { "type": "string" }
    }
  }
}
"#;

const TASKS_SEED: &str = r#"{"title": "Set up Axon", "status": "done", "description": "Install and configure Axon"}
{"title": "Define schema", "status": "in_progress", "description": "Create collection schemas"}
{"title": "Build application", "status": "todo", "description": "Start building on top of Axon"}
"#;

const MAKEFILE: &str = r#".PHONY: serve test seed clean

AXON ?= axon

serve:
	$(AXON) serve --no-auth

seed: schema/tasks.json seed/tasks.jsonl
	$(AXON) collection create tasks --schema schema/tasks.json
	@while IFS= read -r line; do \
		$(AXON) entity create tasks "$$line"; \
	done < seed/tasks.jsonl

test:
	@echo "Creating test entity..."
	$(AXON) entity create tasks '{"title": "smoke test", "status": "todo"}'
	@echo "Listing entities..."
	$(AXON) entity list tasks
	@echo "OK"

clean:
	rm -f axon.db axon-server.db
"#;

const DOCKERFILE: &str = r#"FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=axon:latest /usr/local/bin/axon /usr/local/bin/axon
WORKDIR /app
COPY . .
EXPOSE 4170
CMD ["axon", "serve", "--no-auth"]
"#;

const DOCKER_COMPOSE: &str = r#"services:
  axon:
    build: .
    ports:
      - "4170:4170"
    volumes:
      - ./schema:/app/schema
      - axon-data:/app/data
    environment:
      AXON_NO_AUTH: "true"
      AXON_SQLITE_PATH: /app/data/axon.db
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:4170/healthz"]
      interval: 10s
      timeout: 5s
      retries: 3

volumes:
  axon-data:
"#;

fn readme(name: &str) -> String {
    format!(
        r"# {name}

An Axon-powered application.

## Quick Start

```sh
# Start the server
make serve

# In another terminal, seed the database
make seed

# Run a smoke test
make test
```

## Project Structure

- `schema/` — Collection schemas (JSON)
- `seed/` — Seed data (JSONL)
- `Makefile` — Common development commands
- `Dockerfile` — Container build
- `docker-compose.yml` — Development environment
"
    )
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run_init(name: &str) -> Result<()> {
    let dir = Path::new(name);
    if dir.exists() {
        anyhow::bail!("directory '{}' already exists", name);
    }

    std::fs::create_dir_all(dir.join("schema"))?;
    std::fs::create_dir_all(dir.join("seed"))?;

    std::fs::write(dir.join("schema/tasks.json"), TASKS_SCHEMA)?;
    std::fs::write(dir.join("seed/tasks.jsonl"), TASKS_SEED)?;
    std::fs::write(dir.join("Makefile"), MAKEFILE)?;
    std::fs::write(dir.join("Dockerfile"), DOCKERFILE)?;
    std::fs::write(dir.join("docker-compose.yml"), DOCKER_COMPOSE)?;
    std::fs::write(dir.join("README.md"), readme(name))?;

    println!("Created project '{name}'");
    println!();
    println!("  cd {name}");
    println!("  make serve    # start Axon server");
    println!("  make seed     # load sample data");

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn init_creates_project_structure() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("my-project");
        let name = project.to_str().unwrap();

        run_init(name).unwrap();

        assert!(project.join("schema/tasks.json").exists());
        assert!(project.join("seed/tasks.jsonl").exists());
        assert!(project.join("Makefile").exists());
        assert!(project.join("Dockerfile").exists());
        assert!(project.join("docker-compose.yml").exists());
        assert!(project.join("README.md").exists());

        // Verify schema content is valid JSON.
        let schema_content = fs::read_to_string(project.join("schema/tasks.json")).unwrap();
        let schema: serde_json::Value = serde_json::from_str(&schema_content).unwrap();
        assert_eq!(schema["name"], "tasks");

        // Verify seed content has 3 lines.
        let seed_content = fs::read_to_string(project.join("seed/tasks.jsonl")).unwrap();
        assert_eq!(seed_content.lines().count(), 3);

        // Verify README contains the project name.
        let readme_content = fs::read_to_string(project.join("README.md")).unwrap();
        assert!(readme_content.contains("my-project"));
    }

    #[test]
    fn init_fails_if_directory_exists() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("existing");
        fs::create_dir(&project).unwrap();

        let result = run_init(project.to_str().unwrap());
        assert!(result.is_err());
        assert!(
            result
                .expect_err("init of existing dir should fail")
                .to_string()
                .contains("already exists")
        );
    }
}
