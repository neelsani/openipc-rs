declare module "@tauri-apps/api/core" {
  export class Channel<T = unknown> {
    constructor(onmessage?: (response: T) => void);
  }

  export function invoke<T = unknown>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T>;
}

declare module "@tauri-apps/api/event" {
  export type UnlistenFn = () => void;
  export type Event<T> = {
    event: string;
    id: number;
    payload: T;
  };

  export function listen<T>(
    event: string,
    handler: (event: Event<T>) => void,
  ): Promise<UnlistenFn>;
}
