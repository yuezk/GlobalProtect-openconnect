export type Maybe<T> = T | null | undefined;

export type MaybeProperties<T> = {
  [P in keyof T]?: Maybe<T[P]>;
};
