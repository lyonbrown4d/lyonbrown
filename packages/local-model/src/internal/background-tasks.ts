export const waitForTasks = async (
  tasks: readonly Promise<unknown>[],
  timeoutMs: number,
): Promise<void> => {
  await new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, Math.max(0, timeoutMs));
    void Promise.allSettled(tasks).then(() => {
      clearTimeout(timer);
      resolve();
    });
  });
};
