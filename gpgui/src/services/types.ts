import { Maybe } from '../types';

type PriorityRule = {
  name: Maybe<string>;
  priority: number;
};

export type Gateway = {
  name: Maybe<string>;
  address: Maybe<string>;
  priorityRules: PriorityRule[];
  priority: number;
};
