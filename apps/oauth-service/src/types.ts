export interface BrokerEnv {
  AFS_BROKER_SESSION_SECRET: string;
  AFS_REFRESH_HANDLE_KEY?: string;
  AFS_TOKEN_MODE?: "handle" | "raw";
  AFS_NOTION_CLIENT_ID: string;
  AFS_NOTION_CLIENT_SECRET: string;
  AFS_NOTION_REDIRECT_URIS?: string;
  AFS_NOTION_AUTH_BASE_URL?: string;
  AFS_NOTION_API_BASE_URL?: string;
  AFS_NOTION_VERSION?: string;
}

export type ConnectorId = "notion";

export interface ApiErrorBody {
  error: {
    code: string;
    message: string;
  };
}
