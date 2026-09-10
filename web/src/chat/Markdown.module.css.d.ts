// CSS Modules are typed by hand rather than by a generator, because a generator
// is a build step that has to run before `tsc` and a stale one lies. A class
// this file does not name is a type error, which is the point.
declare const styles: {
  readonly prose: string
  readonly code: string
  readonly fence: string
  readonly link: string
}
export default styles
