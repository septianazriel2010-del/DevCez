export type DeploymentTarget = "preview" | "production";

export interface VercelAccount {
  id: string;
  username: string | null;
  email: string | null;
  name: string | null;
}

export interface VercelConnection {
  connected: boolean;
  tokenSaved: boolean;
  account: VercelAccount | null;
  error: string | null;
}

export interface VercelProject {
  id: string;
  name: string;
  framework: string | null;
  teamId: string | null;
}

export interface ProjectLink {
  projectId: string;
  teamId: string | null;
}

export interface DeploymentProjectInfo {
  path: string;
  name: string;
  framework: string;
  desktopApplication: boolean;
  deployable: boolean;
  validationErrors: string[];
  packageJsonPresent: boolean;
  composerJsonPresent: boolean;
  dependenciesDeclared: boolean;
  lockfilePresent: boolean;
  buildCommand: string | null;
  outputDirectory: string | null;
  installCommand: string | null;
  developmentCommand: string | null;
  environmentStatus: string;
  environmentFiles: string[];
  sensitiveFiles: string[];
  vercelConfigPresent: boolean;
  vercelIgnorePresent: boolean;
  vercelLink: ProjectLink | null;
  cliInstalled: boolean;
  cliVersion: string | null;
  cliError: string | null;
}

export interface ManagedProject {
  path: string;
  accountId: string;
  projectId: string | null;
  projectName: string;
  teamId: string | null;
  createdAt: number;
  creationPending: boolean;
}

export interface DeploymentStartResult {
  runId: string;
  project: ManagedProject;
}

export interface GitCommit {
  hash: string;
  message: string;
  timestamp: number;
}

export interface GitActivity {
  repositoryFound: boolean;
  observedAt: number;
  repositoryRoot: string | null;
  branch: string | null;
  remote: string | null;
  stagedFiles: string[];
  modifiedFiles: string[];
  untrackedFiles: string[];
  ahead: number | null;
  behind: number | null;
  synchronization: string;
  latestCommit: GitCommit | null;
}

export interface DeploymentRecord {
  id: string;
  projectId: string;
  projectName: string;
  url: string | null;
  state: string;
  target: string;
  source: string | null;
  branch: string | null;
  commitSha: string | null;
  commitMessage: string | null;
  createdAt: number;
  readyAt: number | null;
}

export interface DeploymentLog {
  timestamp: number | null;
  kind: string;
  message: string;
}

export interface DeploymentEvent {
  runId: string;
  kind: string;
  stream: string | null;
  message: string;
  projectId: string | null;
}
