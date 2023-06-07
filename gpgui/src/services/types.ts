type PriorityRule = {
  name: string;
  priority: number;
};

export type Gateway = {
  name: string;
  address: string;
  priorityRules: PriorityRule[];
  priority: number;
};
