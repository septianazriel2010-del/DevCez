export type ProjectType =
  | "node_js"
  | "rust"
  | "python"
  | "dot_net"
  | "go"
  | "java_maven"
  | "java_gradle"
  | "cpp_cmake"
  | "static_web"
  | "unknown";

export interface Project {
  id: string;
  name: string;
  path: string;
  project_types: ProjectType[];
}
