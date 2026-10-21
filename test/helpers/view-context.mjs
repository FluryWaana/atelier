export function bindView(factory, context) {
  const dependencies = { ...context, document: { ...context.document, querySelector: context.$ || context.document?.querySelector }, getState: () => context.state };
  for (const name of ['api', 'refresh', 'refreshProcesses']) {
    if (context[name]) dependencies[name] = (...args) => context[name](...args);
  }
  const view = factory(dependencies);
  Object.defineProperties(context, Object.getOwnPropertyDescriptors(view));
  return context;
}
