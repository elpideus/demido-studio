export type Backend = 'cuda' | 'rocm' | 'metal' | 'vulkan' | 'cpu';
export type Scope = 'user' | 'machine';

export interface Gpu {
  vendor: string;
  name: string;
  vram: number;
  integrated: boolean;
  driverVersion: string | null;
}

export interface Hardware {
  os: string;
  osVersion: string;
  cpu: { name: string; physicalCores: number; logicalCores: number };
  totalMemory: number;
  gpus: Gpu[];
}

export interface BackendChoice {
  backend: Backend;
  available: boolean;
  recommended: boolean;
  note: string;
  device: string | null;
  variant: string | null;
  downloadSize: number;
  memoryBudgetGb: number;
  usesSystemMemory: boolean;
}

export interface ModelPick {
  name: string;
  repo: string;
  file: string;
  quant: string;
  size: number;
  /** What lets the model read pictures and sound, downloaded with it. */
  projector?: { file: string; size: number; sha256: string };
}

/** Bytes a pick downloads: the model and its projector. */
export function downloadSize(pick: ModelPick): number {
  return pick.size + (pick.projector?.size ?? 0);
}

export interface Recommendation {
  tier: string;
  contextLength: number;
  picks: Array<[string, ModelPick]>;
  defaultFamily: string;
}

export interface Family {
  id: string;
  label: string;
  vendor: string;
  description: string;
  recommended: boolean;
}

/** How the installed version compares to this setup's: `older` means this setup updates it. */
export type Relation = 'older' | 'same' | 'newer';

export interface ExistingInstall {
  dir: string;
  scope: Scope;
  version: string;
  relation: Relation;
}

/** What `--update` asked for: the installation to update, and whether to start the app afterwards. */
export interface UpdateLaunch {
  dir: string;
  relaunch: boolean;
}

export interface Context {
  version: string;
  mode: 'install' | 'uninstall' | 'update';
  hasPayload: boolean;
  hardware: Hardware;
  choices: BackendChoice[];
  defaultBackend: Backend;
  families: Family[];
  recommendations: Record<string, Recommendation>;
  /** By backend: the search and speech models setup downloads whichever model is picked. */
  helperModels: Record<string, Array<{ label: string; detail: string; size: number }>>;
  defaultDirs: Record<Scope, string>;
  elevated: boolean;
  /** In update mode, the installation at `updateLaunch.dir`, if there is one. */
  existing: ExistingInstall | null;
  resume: WizardState | null;
  fixedSizes: Record<string, number>;
  uninstallDir: string | null;
  updateLaunch: UpdateLaunch | null;
}

export interface DirInfo {
  writable: boolean;
  freeBytes: number | null;
  hasInstall: boolean;
  running: boolean;
  /** Holds an install.json setup cannot read, which it must not overwrite. */
  hasUnreadableInstall: boolean;
  /** Holds files that are not a Demido Studio installation. */
  hasOtherFiles: boolean;
  /** Is, holds or sits inside the folder with the person's chats and models. */
  overlapsUserData: boolean;
  /** Why setup must not install here; blocks the installation. */
  problem: string | null;
}

/** What the installation writes to one volume, next to what is free there. */
export interface VolumeNeed {
  mount: string;
  freeBytes: number;
  neededBytes: number;
}

/** Everything the person chose; handed over when the wizard restarts elevated. */
export interface WizardState {
  step: number;
  backend: Backend;
  scope: Scope;
  installDir: string;
  customDir: boolean;
  family: string | null;
  shortcuts: boolean;
}

export type StepId =
  | 'app'
  | 'runtime'
  | 'node'
  | 'uv'
  | 'python'
  | 'pythonPackages'
  | 'model'
  | 'searchModel'
  | 'speechModel'
  | 'shortcuts'
  | 'finalize';
export type StepState = 'pending' | 'running' | 'done' | 'failed' | 'skipped';

export interface StepInfo {
  id: StepId;
  label: string;
  detail: string;
  size: number;
}

export type ProvisionEvent =
  | { type: 'plan'; steps: StepInfo[] }
  | {
      type: 'step';
      id: StepId;
      state: StepState;
      message?: string;
      /** On the app step's failure: it could not undo all its changes, so the install folder holds
       * part of each version. Running setup again finishes the update. */
      incomplete?: boolean;
    }
  | { type: 'progress'; id: StepId; done: number; total: number | null; bytesPerSecond: number; activity: string }
  | { type: 'log'; id: StepId; line: string }
  | { type: 'finished'; success: boolean; failed: StepId[] };
