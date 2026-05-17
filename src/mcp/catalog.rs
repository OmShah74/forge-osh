//! Built-in catalog of supported MCP services.
//!
//! Each entry describes how to launch a public MCP server (the command and
//! args), and what secrets it needs from the user. The TUI Mcp Manager modal
//! reads this catalog to render its list of "available services" and to
//! prompt for the right secrets.
//!
//! Users can also add bespoke servers from outside this catalog via config
//! file — see `McpServerConfig::custom`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretSpec {
    /// Unique key under the server (becomes env var name and storage key).
    pub key: String,
    /// Human-friendly label shown in the TUI prompt.
    pub label: String,
    /// Where the user obtains this secret (printable URL or hint).
    pub help: String,
    /// If true, secret is required for the server to run.
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub secrets: &'static [(&'static str, &'static str, &'static str, bool)],
}

impl CatalogEntry {
    pub fn secret_specs(&self) -> Vec<SecretSpec> {
        self.secrets
            .iter()
            .map(|(k, l, h, r)| SecretSpec {
                key: (*k).to_string(),
                label: (*l).to_string(),
                help: (*h).to_string(),
                required: *r,
            })
            .collect()
    }
}

/// Built-in catalog. The command/args follow each server's published install
/// instructions. Anything Node-based uses `npx -y <pkg>`; Python uses
/// `uvx <pkg>`. Users without `npx`/`uvx` will see a clear spawn error.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "github",
        display_name: "GitHub",
        description: "Repos, issues, PRs, code search via the official GitHub MCP server",
        category: "Dev platform",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        secrets: &[(
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "Personal Access Token",
            "Create at https://github.com/settings/tokens (scopes: repo, read:org)",
            true,
        )],
    },
    CatalogEntry {
        id: "gitlab",
        display_name: "GitLab",
        description: "Projects, issues, merge requests via the official GitLab MCP server",
        category: "Dev platform",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-gitlab"],
        secrets: &[
            (
                "GITLAB_PERSONAL_ACCESS_TOKEN",
                "Personal Access Token",
                "Create at https://gitlab.com/-/user_settings/personal_access_tokens",
                true,
            ),
            (
                "GITLAB_API_URL",
                "API URL (optional, for self-hosted)",
                "e.g. https://gitlab.example.com/api/v4",
                false,
            ),
        ],
    },
    CatalogEntry {
        id: "filesystem",
        display_name: "Filesystem (sandboxed)",
        description: "Read/write within an explicit allowlist of directories",
        category: "Files",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-filesystem"],
        secrets: &[(
            "MCP_FS_ROOT",
            "Allowed root directory",
            "Absolute path the server may access (passed as first arg)",
            true,
        )],
    },
    CatalogEntry {
        id: "postgres",
        display_name: "PostgreSQL",
        description: "Read-only SQL queries against a Postgres database",
        category: "Database",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-postgres"],
        secrets: &[(
            "POSTGRES_CONNECTION_STRING",
            "Connection URL",
            "postgres://user:pass@host:5432/db",
            true,
        )],
    },
    CatalogEntry {
        id: "sqlite",
        display_name: "SQLite",
        description: "Query a local SQLite database file",
        category: "Database",
        command: "uvx",
        args: &["mcp-server-sqlite"],
        secrets: &[(
            "SQLITE_DB_PATH",
            "SQLite database path",
            "Absolute path to the .db / .sqlite file",
            true,
        )],
    },
    CatalogEntry {
        id: "slack",
        display_name: "Slack",
        description: "Read messages, post to channels, manage workspace",
        category: "Communication",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-slack"],
        secrets: &[
            (
                "SLACK_BOT_TOKEN",
                "Bot User OAuth Token (xoxb-…)",
                "https://api.slack.com/apps → OAuth & Permissions",
                true,
            ),
            (
                "SLACK_TEAM_ID",
                "Team / Workspace ID",
                "Found in Slack URL or Workspace settings",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "google-drive",
        display_name: "Google Drive",
        description: "List files, fetch contents from Drive",
        category: "Files",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-gdrive"],
        secrets: &[(
            "GDRIVE_CREDENTIALS_PATH",
            "Path to OAuth credentials JSON",
            "Download from Google Cloud Console; first run will open a browser",
            true,
        )],
    },
    CatalogEntry {
        id: "google-maps",
        display_name: "Google Maps",
        description: "Geocoding, directions, places",
        category: "Location",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-google-maps"],
        secrets: &[(
            "GOOGLE_MAPS_API_KEY",
            "Maps Platform API key",
            "https://console.cloud.google.com/google/maps-apis",
            true,
        )],
    },
    CatalogEntry {
        id: "brave-search",
        display_name: "Brave Search",
        description: "Web search via the Brave Search API",
        category: "Search",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-brave-search"],
        secrets: &[(
            "BRAVE_API_KEY",
            "Brave Search API key",
            "https://api.search.brave.com/app/keys",
            true,
        )],
    },
    CatalogEntry {
        id: "puppeteer",
        display_name: "Puppeteer (browser)",
        description: "Headless Chrome — navigate, screenshot, interact",
        category: "Browser",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-puppeteer"],
        secrets: &[],
    },
    CatalogEntry {
        id: "memory",
        display_name: "Memory (knowledge graph)",
        description: "Persistent knowledge graph the model can read & write",
        category: "Memory",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        secrets: &[],
    },
    CatalogEntry {
        id: "fetch",
        display_name: "Fetch",
        description: "Generic HTTP fetch with markdown extraction",
        category: "Web",
        command: "uvx",
        args: &["mcp-server-fetch"],
        secrets: &[],
    },
    CatalogEntry {
        id: "time",
        display_name: "Time / Timezone",
        description: "Current time, timezone conversions, scheduling helpers",
        category: "Utilities",
        command: "uvx",
        args: &["mcp-server-time"],
        secrets: &[],
    },
    CatalogEntry {
        id: "sequential-thinking",
        display_name: "Sequential Thinking",
        description: "Structured step-by-step reasoning scratchpad",
        category: "Reasoning",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sequential-thinking"],
        secrets: &[],
    },
    CatalogEntry {
        id: "everart",
        display_name: "EverArt (image gen)",
        description: "Image generation via the EverArt API",
        category: "Media",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-everart"],
        secrets: &[(
            "EVERART_API_KEY",
            "EverArt API Key",
            "https://www.everart.ai/api",
            true,
        )],
    },
    CatalogEntry {
        id: "sentry",
        display_name: "Sentry",
        description: "Issues, events, releases via the Sentry MCP server",
        category: "Observability",
        command: "uvx",
        args: &["mcp-server-sentry"],
        secrets: &[(
            "SENTRY_AUTH_TOKEN",
            "Sentry Auth Token",
            "https://sentry.io/settings/account/api/auth-tokens/",
            true,
        )],
    },
    // ── Dev platforms ─────────────────────────────────────────────────────
    CatalogEntry {
        id: "linear",
        display_name: "Linear",
        description: "Issues, projects, cycles via Linear's MCP server",
        category: "Dev platform",
        command: "npx",
        args: &["-y", "@tacticlaunch/mcp-linear"],
        secrets: &[(
            "LINEAR_API_KEY",
            "Linear API Key",
            "https://linear.app/settings/api",
            true,
        )],
    },
    CatalogEntry {
        id: "jira",
        display_name: "Jira",
        description: "Atlassian Jira issues, JQL search, transitions",
        category: "Dev platform",
        command: "uvx",
        args: &["mcp-atlassian"],
        secrets: &[
            (
                "JIRA_URL",
                "Jira site URL",
                "e.g. https://acme.atlassian.net",
                true,
            ),
            ("JIRA_USERNAME", "Email", "Atlassian account email", true),
            (
                "JIRA_API_TOKEN",
                "API Token",
                "https://id.atlassian.com/manage-profile/security/api-tokens",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "confluence",
        display_name: "Confluence",
        description: "Atlassian Confluence pages, spaces, search",
        category: "Docs",
        command: "uvx",
        args: &["mcp-atlassian"],
        secrets: &[
            (
                "CONFLUENCE_URL",
                "Confluence URL",
                "https://acme.atlassian.net/wiki",
                true,
            ),
            (
                "CONFLUENCE_USERNAME",
                "Email",
                "Atlassian account email",
                true,
            ),
            (
                "CONFLUENCE_API_TOKEN",
                "API Token",
                "https://id.atlassian.com/manage-profile/security/api-tokens",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "notion",
        display_name: "Notion",
        description: "Read/write Notion pages, databases, blocks",
        category: "Docs",
        command: "npx",
        args: &["-y", "@notionhq/notion-mcp-server"],
        secrets: &[(
            "NOTION_API_KEY",
            "Internal Integration Token",
            "https://www.notion.so/profile/integrations",
            true,
        )],
    },
    CatalogEntry {
        id: "asana",
        display_name: "Asana",
        description: "Tasks, projects, workspaces",
        category: "Dev platform",
        command: "npx",
        args: &["-y", "@cristip73/mcp-server-asana"],
        secrets: &[(
            "ASANA_ACCESS_TOKEN",
            "Personal Access Token",
            "https://app.asana.com/0/my-apps",
            true,
        )],
    },
    CatalogEntry {
        id: "trello",
        display_name: "Trello",
        description: "Boards, lists, cards via the Trello REST API",
        category: "Dev platform",
        command: "npx",
        args: &["-y", "@delorenj/mcp-server-trello"],
        secrets: &[
            (
                "TRELLO_API_KEY",
                "API Key",
                "https://trello.com/power-ups/admin",
                true,
            ),
            (
                "TRELLO_TOKEN",
                "Token",
                "Generate from the same admin page",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "bitbucket",
        display_name: "Bitbucket",
        description: "Repos, PRs, pipelines on Bitbucket Cloud",
        category: "Dev platform",
        command: "npx",
        args: &["-y", "@nclud/mcp-bitbucket"],
        secrets: &[
            (
                "BITBUCKET_USERNAME",
                "Username",
                "Bitbucket account username",
                true,
            ),
            (
                "BITBUCKET_APP_PASSWORD",
                "App Password",
                "https://bitbucket.org/account/settings/app-passwords/",
                true,
            ),
        ],
    },
    // ── Databases ─────────────────────────────────────────────────────────
    CatalogEntry {
        id: "mysql",
        display_name: "MySQL",
        description: "Read-only SQL queries against MySQL/MariaDB",
        category: "Database",
        command: "npx",
        args: &["-y", "@benborla29/mcp-server-mysql"],
        secrets: &[(
            "MYSQL_CONNECTION_STRING",
            "Connection URL",
            "mysql://user:pass@host:3306/db",
            true,
        )],
    },
    CatalogEntry {
        id: "mongodb",
        display_name: "MongoDB",
        description: "MongoDB queries via the official MongoDB MCP server",
        category: "Database",
        command: "npx",
        args: &["-y", "mongodb-mcp-server"],
        secrets: &[(
            "MDB_MCP_CONNECTION_STRING",
            "Connection URI",
            "mongodb+srv://user:pass@cluster/db",
            true,
        )],
    },
    CatalogEntry {
        id: "redis",
        display_name: "Redis",
        description: "Get/set/list keys, TTLs, pub/sub via Redis MCP server",
        category: "Database",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-redis"],
        secrets: &[(
            "REDIS_URL",
            "Connection URL",
            "redis://[user:pass@]host:6379[/db]",
            true,
        )],
    },
    CatalogEntry {
        id: "bigquery",
        display_name: "BigQuery",
        description: "Run SQL against Google BigQuery datasets",
        category: "Database",
        command: "uvx",
        args: &["mcp-server-bigquery"],
        secrets: &[
            (
                "BIGQUERY_PROJECT_ID",
                "GCP Project ID",
                "Project containing the datasets",
                true,
            ),
            (
                "GOOGLE_APPLICATION_CREDENTIALS",
                "Service Account JSON path",
                "Absolute path to a service account key file",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "snowflake",
        display_name: "Snowflake",
        description: "Snowflake SQL queries via MCP",
        category: "Database",
        command: "uvx",
        args: &["mcp_snowflake_server"],
        secrets: &[
            (
                "SNOWFLAKE_ACCOUNT",
                "Account",
                "e.g. xy12345.us-east-1",
                true,
            ),
            ("SNOWFLAKE_USER", "Username", "Snowflake username", true),
            ("SNOWFLAKE_PASSWORD", "Password", "Snowflake password", true),
            (
                "SNOWFLAKE_WAREHOUSE",
                "Warehouse",
                "Compute warehouse name",
                true,
            ),
            ("SNOWFLAKE_DATABASE", "Database", "Default database", false),
        ],
    },
    CatalogEntry {
        id: "clickhouse",
        display_name: "ClickHouse",
        description: "Run analytical SQL on ClickHouse",
        category: "Database",
        command: "uvx",
        args: &["mcp-clickhouse"],
        secrets: &[
            (
                "CLICKHOUSE_HOST",
                "Host",
                "e.g. localhost or play.clickhouse.com",
                true,
            ),
            (
                "CLICKHOUSE_PORT",
                "Port",
                "8443 for HTTPS, 9000 for native",
                false,
            ),
            ("CLICKHOUSE_USER", "User", "ClickHouse user", true),
            (
                "CLICKHOUSE_PASSWORD",
                "Password",
                "ClickHouse password",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "duckdb",
        display_name: "DuckDB",
        description: "Local analytical SQL on Parquet/CSV/SQLite via DuckDB",
        category: "Database",
        command: "uvx",
        args: &["mcp-server-motherduck"],
        secrets: &[(
            "MOTHERDUCK_TOKEN",
            "MotherDuck token (optional)",
            "https://app.motherduck.com — leave empty for local DuckDB only",
            false,
        )],
    },
    CatalogEntry {
        id: "supabase",
        display_name: "Supabase",
        description: "Postgres tables, auth, storage on a Supabase project",
        category: "Database",
        command: "npx",
        args: &["-y", "@supabase/mcp-server-supabase"],
        secrets: &[(
            "SUPABASE_ACCESS_TOKEN",
            "Personal Access Token",
            "https://supabase.com/dashboard/account/tokens",
            true,
        )],
    },
    CatalogEntry {
        id: "neon",
        display_name: "Neon (Postgres)",
        description: "Manage Neon serverless Postgres branches and queries",
        category: "Database",
        command: "npx",
        args: &["-y", "@neondatabase/mcp-server-neon"],
        secrets: &[(
            "NEON_API_KEY",
            "Neon API Key",
            "https://console.neon.tech/app/settings/api-keys",
            true,
        )],
    },
    CatalogEntry {
        id: "planetscale",
        display_name: "PlanetScale",
        description: "PlanetScale databases, branches, deploys",
        category: "Database",
        command: "npx",
        args: &["-y", "@planetscale/mcp-server"],
        secrets: &[
            (
                "PLANETSCALE_SERVICE_TOKEN_ID",
                "Service Token ID",
                "https://app.planetscale.com/<org>/settings/service-tokens",
                true,
            ),
            (
                "PLANETSCALE_SERVICE_TOKEN",
                "Service Token",
                "Token value generated next to the ID",
                true,
            ),
        ],
    },
    // ── Vector / search ──────────────────────────────────────────────────
    CatalogEntry {
        id: "elasticsearch",
        display_name: "Elasticsearch",
        description: "Search and aggregations on an Elastic cluster",
        category: "Search",
        command: "uvx",
        args: &["mcp-server-elasticsearch"],
        secrets: &[
            ("ES_URL", "Cluster URL", "https://es.example.com:9200", true),
            (
                "ES_API_KEY",
                "API Key (optional)",
                "Use API key OR user/pass",
                false,
            ),
            (
                "ES_USERNAME",
                "Username (optional)",
                "Basic auth username",
                false,
            ),
            (
                "ES_PASSWORD",
                "Password (optional)",
                "Basic auth password",
                false,
            ),
        ],
    },
    CatalogEntry {
        id: "pinecone",
        display_name: "Pinecone",
        description: "Vector search over Pinecone indexes",
        category: "Vector",
        command: "uvx",
        args: &["mcp-pinecone"],
        secrets: &[
            (
                "PINECONE_API_KEY",
                "API Key",
                "https://app.pinecone.io",
                true,
            ),
            (
                "PINECONE_INDEX_NAME",
                "Default Index",
                "Name of an existing index",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "qdrant",
        display_name: "Qdrant",
        description: "Vector search over a Qdrant collection",
        category: "Vector",
        command: "uvx",
        args: &["mcp-server-qdrant"],
        secrets: &[
            (
                "QDRANT_URL",
                "Cluster URL",
                "http://localhost:6333 or hosted URL",
                true,
            ),
            (
                "QDRANT_API_KEY",
                "API Key (optional)",
                "Required for Qdrant Cloud",
                false,
            ),
            (
                "COLLECTION_NAME",
                "Default Collection",
                "Existing collection to search/upsert",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "chroma",
        display_name: "Chroma",
        description: "Local Chroma vector DB",
        category: "Vector",
        command: "uvx",
        args: &["chroma-mcp"],
        secrets: &[(
            "CHROMA_DB_PATH",
            "DB Path",
            "Absolute path to the chroma persistence directory",
            true,
        )],
    },
    CatalogEntry {
        id: "tavily",
        display_name: "Tavily Search",
        description: "AI-tuned web search via the Tavily API",
        category: "Search",
        command: "npx",
        args: &["-y", "tavily-mcp"],
        secrets: &[(
            "TAVILY_API_KEY",
            "Tavily API Key",
            "https://app.tavily.com",
            true,
        )],
    },
    CatalogEntry {
        id: "exa",
        display_name: "Exa Search",
        description: "Neural web search & content retrieval via Exa",
        category: "Search",
        command: "npx",
        args: &["-y", "exa-mcp-server"],
        secrets: &[(
            "EXA_API_KEY",
            "Exa API Key",
            "https://dashboard.exa.ai/api-keys",
            true,
        )],
    },
    CatalogEntry {
        id: "perplexity",
        display_name: "Perplexity",
        description: "Perplexity online search/chat completions",
        category: "Search",
        command: "npx",
        args: &["-y", "@perplexity-ai/mcp-server"],
        secrets: &[(
            "PERPLEXITY_API_KEY",
            "Perplexity API Key",
            "https://www.perplexity.ai/settings/api",
            true,
        )],
    },
    CatalogEntry {
        id: "kagi",
        display_name: "Kagi Search",
        description: "Kagi search & summarizer APIs",
        category: "Search",
        command: "uvx",
        args: &["kagimcp"],
        secrets: &[(
            "KAGI_API_KEY",
            "Kagi API Key",
            "https://kagi.com/settings?p=api",
            true,
        )],
    },
    // ── Communication ────────────────────────────────────────────────────
    CatalogEntry {
        id: "discord",
        display_name: "Discord",
        description: "Read/post messages, manage channels via a Discord bot token",
        category: "Communication",
        command: "npx",
        args: &["-y", "discord-mcp"],
        secrets: &[(
            "DISCORD_BOT_TOKEN",
            "Bot Token",
            "https://discord.com/developers/applications",
            true,
        )],
    },
    CatalogEntry {
        id: "telegram",
        display_name: "Telegram",
        description: "Send and read Telegram messages via Bot API",
        category: "Communication",
        command: "npx",
        args: &["-y", "@chigwell/telegram-mcp"],
        secrets: &[(
            "TELEGRAM_BOT_TOKEN",
            "Bot Token",
            "Talk to @BotFather in Telegram to create a bot",
            true,
        )],
    },
    CatalogEntry {
        id: "twilio",
        display_name: "Twilio (SMS)",
        description: "Send SMS, MMS, and WhatsApp messages via Twilio",
        category: "Communication",
        command: "npx",
        args: &["-y", "@twilio-alpha/mcp"],
        secrets: &[
            (
                "TWILIO_ACCOUNT_SID",
                "Account SID",
                "https://console.twilio.com",
                true,
            ),
            (
                "TWILIO_AUTH_TOKEN",
                "Auth Token",
                "Same console — Account Info",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "gmail",
        display_name: "Gmail",
        description: "Search, read, send Gmail messages via OAuth",
        category: "Communication",
        command: "npx",
        args: &["-y", "@gongrzhe/server-gmail-autoauth-mcp"],
        secrets: &[(
            "GMAIL_CREDENTIALS_PATH",
            "OAuth credentials JSON path",
            "Download from Google Cloud Console; first run opens browser",
            true,
        )],
    },
    CatalogEntry {
        id: "google-calendar",
        display_name: "Google Calendar",
        description: "Events, schedules, attendees on Google Calendar",
        category: "Productivity",
        command: "npx",
        args: &["-y", "@cocal/google-calendar-mcp"],
        secrets: &[(
            "GOOGLE_CREDENTIALS_PATH",
            "OAuth credentials JSON path",
            "Download from Google Cloud Console; first run opens browser",
            true,
        )],
    },
    // ── Cloud / infra ────────────────────────────────────────────────────
    CatalogEntry {
        id: "cloudflare",
        display_name: "Cloudflare",
        description: "Workers, KV, R2, D1 management via the Cloudflare MCP server",
        category: "Cloud",
        command: "npx",
        args: &["-y", "@cloudflare/mcp-server-cloudflare"],
        secrets: &[
            (
                "CLOUDFLARE_API_TOKEN",
                "API Token",
                "https://dash.cloudflare.com/profile/api-tokens",
                true,
            ),
            (
                "CLOUDFLARE_ACCOUNT_ID",
                "Account ID",
                "Right sidebar of the Cloudflare dashboard",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "aws-kb",
        display_name: "AWS Knowledge Base",
        description: "AWS Bedrock Knowledge Base retrieval (RAG over your docs)",
        category: "Cloud",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-aws-kb-retrieval"],
        secrets: &[
            (
                "AWS_ACCESS_KEY_ID",
                "Access Key ID",
                "IAM credentials",
                true,
            ),
            (
                "AWS_SECRET_ACCESS_KEY",
                "Secret Access Key",
                "IAM credentials",
                true,
            ),
            ("AWS_REGION", "Region", "e.g. us-east-1", true),
        ],
    },
    CatalogEntry {
        id: "vercel",
        display_name: "Vercel",
        description: "Projects, deployments, env vars on Vercel",
        category: "Cloud",
        command: "npx",
        args: &["-y", "@vercel/mcp"],
        secrets: &[(
            "VERCEL_API_TOKEN",
            "Vercel API Token",
            "https://vercel.com/account/tokens",
            true,
        )],
    },
    CatalogEntry {
        id: "netlify",
        display_name: "Netlify",
        description: "Sites, deploys, build hooks on Netlify",
        category: "Cloud",
        command: "npx",
        args: &["-y", "@netlify/mcp"],
        secrets: &[(
            "NETLIFY_AUTH_TOKEN",
            "Personal Access Token",
            "https://app.netlify.com/user/applications#personal-access-tokens",
            true,
        )],
    },
    CatalogEntry {
        id: "railway",
        display_name: "Railway",
        description: "Projects, services, deploys on Railway",
        category: "Cloud",
        command: "npx",
        args: &["-y", "@jasontanswe/railway-mcp"],
        secrets: &[(
            "RAILWAY_API_TOKEN",
            "API Token",
            "https://railway.app/account/tokens",
            true,
        )],
    },
    CatalogEntry {
        id: "docker",
        display_name: "Docker",
        description: "List/start/stop containers, build images via the local Docker daemon",
        category: "Infra",
        command: "uvx",
        args: &["docker-mcp"],
        secrets: &[],
    },
    CatalogEntry {
        id: "kubernetes",
        display_name: "Kubernetes",
        description: "Read/manage pods, deployments via your kubeconfig",
        category: "Infra",
        command: "npx",
        args: &["-y", "mcp-server-kubernetes"],
        secrets: &[(
            "KUBECONFIG",
            "kubeconfig path (optional)",
            "Defaults to ~/.kube/config if empty",
            false,
        )],
    },
    // ── Commerce / fintech ───────────────────────────────────────────────
    CatalogEntry {
        id: "stripe",
        display_name: "Stripe",
        description: "Customers, charges, subscriptions on Stripe",
        category: "Commerce",
        command: "npx",
        args: &["-y", "@stripe/mcp"],
        secrets: &[(
            "STRIPE_SECRET_KEY",
            "Secret Key",
            "https://dashboard.stripe.com/apikeys",
            true,
        )],
    },
    CatalogEntry {
        id: "shopify",
        display_name: "Shopify",
        description: "Products, orders, customers on a Shopify store",
        category: "Commerce",
        command: "npx",
        args: &["-y", "shopify-mcp-server"],
        secrets: &[
            (
                "SHOPIFY_SHOP",
                "Shop domain",
                "e.g. mystore.myshopify.com",
                true,
            ),
            (
                "SHOPIFY_ACCESS_TOKEN",
                "Admin API access token",
                "Generated when installing a custom app",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "hubspot",
        display_name: "HubSpot",
        description: "Contacts, companies, deals via HubSpot CRM",
        category: "CRM",
        command: "npx",
        args: &["-y", "@hubspot/mcp-server"],
        secrets: &[(
            "HUBSPOT_ACCESS_TOKEN",
            "Private App Token",
            "https://app.hubspot.com/private-apps",
            true,
        )],
    },
    CatalogEntry {
        id: "salesforce",
        display_name: "Salesforce",
        description: "SOQL queries, records, dashboards on Salesforce",
        category: "CRM",
        command: "uvx",
        args: &["mcp-salesforce"],
        secrets: &[
            (
                "SALESFORCE_USERNAME",
                "Username",
                "Salesforce login email",
                true,
            ),
            (
                "SALESFORCE_PASSWORD",
                "Password",
                "Salesforce password",
                true,
            ),
            (
                "SALESFORCE_SECURITY_TOKEN",
                "Security Token",
                "Reset under Personal Settings → My Security Token",
                true,
            ),
        ],
    },
    // ── Media / social ───────────────────────────────────────────────────
    CatalogEntry {
        id: "spotify",
        display_name: "Spotify",
        description: "Playback control, search, playlists on Spotify",
        category: "Media",
        command: "npx",
        args: &["-y", "@hardchor/spotify-mcp"],
        secrets: &[
            (
                "SPOTIFY_CLIENT_ID",
                "Client ID",
                "https://developer.spotify.com/dashboard",
                true,
            ),
            (
                "SPOTIFY_CLIENT_SECRET",
                "Client Secret",
                "Same dashboard",
                true,
            ),
        ],
    },
    CatalogEntry {
        id: "youtube",
        display_name: "YouTube",
        description: "Search videos, fetch transcripts, channel metadata",
        category: "Media",
        command: "npx",
        args: &["-y", "@anaisbetts/mcp-youtube"],
        secrets: &[(
            "YOUTUBE_API_KEY",
            "YouTube Data API key",
            "https://console.cloud.google.com/apis/credentials",
            true,
        )],
    },
    CatalogEntry {
        id: "reddit",
        display_name: "Reddit",
        description: "Subreddit feeds, post search, comment fetch",
        category: "Social",
        command: "uvx",
        args: &["mcp-reddit"],
        secrets: &[
            (
                "REDDIT_CLIENT_ID",
                "Client ID",
                "https://www.reddit.com/prefs/apps",
                true,
            ),
            (
                "REDDIT_CLIENT_SECRET",
                "Client Secret",
                "Same apps page",
                true,
            ),
            ("REDDIT_USERNAME", "Username", "Your Reddit username", true),
            ("REDDIT_PASSWORD", "Password", "Your Reddit password", true),
        ],
    },
    CatalogEntry {
        id: "x-twitter",
        display_name: "X / Twitter",
        description: "Read & post tweets via the X API v2",
        category: "Social",
        command: "uvx",
        args: &["mcp-twitter"],
        secrets: &[(
            "X_BEARER_TOKEN",
            "Bearer Token",
            "https://developer.x.com/en/portal/dashboard",
            true,
        )],
    },
    // ── Productivity / knowledge ─────────────────────────────────────────
    CatalogEntry {
        id: "obsidian",
        display_name: "Obsidian (vault)",
        description: "Read/write notes in a local Obsidian vault",
        category: "Knowledge",
        command: "uvx",
        args: &["mcp-obsidian"],
        secrets: &[(
            "OBSIDIAN_VAULT_PATH",
            "Vault directory",
            "Absolute path to your Obsidian vault",
            true,
        )],
    },
    CatalogEntry {
        id: "wikipedia",
        display_name: "Wikipedia",
        description: "Search and fetch Wikipedia articles",
        category: "Knowledge",
        command: "uvx",
        args: &["mcp-wikipedia"],
        secrets: &[],
    },
    CatalogEntry {
        id: "arxiv",
        display_name: "arXiv",
        description: "Search and download papers from arXiv",
        category: "Knowledge",
        command: "uvx",
        args: &["arxiv-mcp-server"],
        secrets: &[(
            "ARXIV_STORAGE_PATH",
            "Local storage dir",
            "Where downloaded PDFs are cached (default ~/.arxiv-cache)",
            false,
        )],
    },
    CatalogEntry {
        id: "hackernews",
        display_name: "Hacker News",
        description: "Top/new/best stories, item lookup, user profiles",
        category: "Knowledge",
        command: "uvx",
        args: &["mcp-hn"],
        secrets: &[],
    },
    // ── Design / creative ────────────────────────────────────────────────
    CatalogEntry {
        id: "figma",
        display_name: "Figma",
        description: "Read Figma files, components, variables",
        category: "Design",
        command: "npx",
        args: &["-y", "figma-developer-mcp"],
        secrets: &[(
            "FIGMA_ACCESS_TOKEN",
            "Personal Access Token",
            "https://www.figma.com/settings — Personal access tokens",
            true,
        )],
    },
    // ── Data / utilities ─────────────────────────────────────────────────
    CatalogEntry {
        id: "openweather",
        display_name: "OpenWeather",
        description: "Current weather and forecasts via OpenWeatherMap",
        category: "Data",
        command: "uvx",
        args: &["mcp-openweather"],
        secrets: &[(
            "OPENWEATHER_API_KEY",
            "API Key",
            "https://home.openweathermap.org/api_keys",
            true,
        )],
    },
    CatalogEntry {
        id: "alpha-vantage",
        display_name: "Alpha Vantage (stocks)",
        description: "Stock quotes, fundamentals, FX rates",
        category: "Data",
        command: "uvx",
        args: &["mcp-alpha-vantage"],
        secrets: &[(
            "ALPHAVANTAGE_API_KEY",
            "API Key",
            "https://www.alphavantage.co/support/#api-key",
            true,
        )],
    },
    CatalogEntry {
        id: "newsapi",
        display_name: "NewsAPI",
        description: "Top headlines and article search via NewsAPI",
        category: "Data",
        command: "uvx",
        args: &["mcp-newsapi"],
        secrets: &[(
            "NEWSAPI_KEY",
            "NewsAPI Key",
            "https://newsapi.org/account",
            true,
        )],
    },
    CatalogEntry {
        id: "duckduckgo",
        display_name: "DuckDuckGo Search",
        description: "Free instant-answers and web search via DuckDuckGo",
        category: "Search",
        command: "uvx",
        args: &["duckduckgo-mcp-server"],
        secrets: &[],
    },
    CatalogEntry {
        id: "git-local",
        display_name: "Git (local repo)",
        description: "Operate on a local git repository (status/log/diff/show)",
        category: "Files",
        command: "uvx",
        args: &["mcp-server-git"],
        secrets: &[(
            "GIT_REPOSITORY",
            "Repository path",
            "Absolute path to the .git working tree",
            true,
        )],
    },
];

pub fn lookup(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn catalog_has_expected_builtin_coverage() {
        assert!(
            CATALOG.len() >= 50,
            "expected 50+ MCP catalog entries, got {}",
            CATALOG.len()
        );
    }

    #[test]
    fn catalog_ids_are_unique_and_lookup_round_trips() {
        let mut seen = HashSet::new();
        for entry in CATALOG {
            assert!(!entry.id.trim().is_empty());
            assert!(
                entry
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
                "catalog id '{}' is not CLI/config friendly",
                entry.id
            );
            assert!(seen.insert(entry.id), "duplicate catalog id '{}'", entry.id);
            assert_eq!(
                lookup(entry.id).map(|e| e.display_name),
                Some(entry.display_name)
            );
        }
    }

    #[test]
    fn catalog_entries_have_launch_metadata() {
        for entry in CATALOG {
            assert!(
                !entry.display_name.trim().is_empty(),
                "{} has empty display name",
                entry.id
            );
            assert!(
                !entry.description.trim().is_empty(),
                "{} has empty description",
                entry.id
            );
            assert!(
                !entry.category.trim().is_empty(),
                "{} has empty category",
                entry.id
            );
            assert!(
                !entry.command.trim().is_empty(),
                "{} has empty command",
                entry.id
            );
            assert!(
                !entry.args.iter().any(|arg| arg.trim().is_empty()),
                "{} has empty launch arg",
                entry.id
            );
        }
    }

    #[test]
    fn catalog_secret_specs_are_env_var_shaped() {
        for entry in CATALOG {
            let mut seen = HashSet::new();
            for (key, label, help, _) in entry.secrets {
                assert!(!key.trim().is_empty(), "{} has empty secret key", entry.id);
                assert!(
                    key.chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                    "{} secret '{}' is not env-var shaped",
                    entry.id,
                    key
                );
                assert!(
                    seen.insert(*key),
                    "{} has duplicate secret '{}'",
                    entry.id,
                    key
                );
                assert!(
                    !label.trim().is_empty(),
                    "{} secret '{}' has empty label",
                    entry.id,
                    key
                );
                assert!(
                    !help.trim().is_empty(),
                    "{} secret '{}' has empty help",
                    entry.id,
                    key
                );
            }
        }
    }
}
