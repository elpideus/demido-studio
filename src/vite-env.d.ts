/// <reference types="vite/client" />

declare const __APP_VERSION__: string

declare module 'file-icons-js' {
  export function getClass(name: string): string | null
  export function getClassWithColor(name: string): string | null
}
