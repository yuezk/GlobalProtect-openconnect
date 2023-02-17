import { Maybe } from '../types';

type PriorityRule = {
  name: Maybe<string>;
  priority: Maybe<number>;
};

export type Gateway = {
  name: Maybe<string>;
  address: Maybe<string>;
  priorityRules: PriorityRule[];
  priority: Maybe<number>;
};
