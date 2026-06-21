import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function clampToViewport(el: HTMLElement) {
  const rect = el.getBoundingClientRect()
  const pad = 8
  let { left, top } = rect

  if (rect.right > window.innerWidth - pad) {
    left = window.innerWidth - rect.width - pad
  }
  if (rect.bottom > window.innerHeight - pad) {
    top = window.innerHeight - rect.height - pad
  }
  if (left < pad) left = pad
  if (top < pad) top = pad

  if (left !== rect.left || top !== rect.top) {
    el.style.left = `${left}px`
    el.style.top = `${top}px`
  }
}
