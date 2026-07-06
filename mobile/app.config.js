module.exports = ({ config }) => {
  const extra = config.extra || {};
  const eas = extra.eas || {};
  const ios = config.ios || {};
  const slug = config.slug;
  const projectId = process.env.EAS_PROJECT_ID || eas.projectId;
  const buildNumber = process.env.IOS_BUILD_NUMBER || ios.buildNumber || "1";
  const plugins = withPlugins(config.plugins || [], [
    "expo-background-task",
    "expo-status-bar",
  ]);

  return {
    ...config,
    slug,
    plugins,
    ios: {
      ...ios,
      buildNumber,
    },
    extra: {
      ...extra,
      eas: projectId
        ? {
            ...eas,
            projectId,
          }
        : eas,
    },
  };
};

function withPlugins(plugins, pluginNames) {
  return pluginNames.reduce((nextPlugins, pluginName) => {
    const hasPlugin = nextPlugins.some((plugin) =>
      Array.isArray(plugin) ? plugin[0] === pluginName : plugin === pluginName
    );

    return hasPlugin ? nextPlugins : [...nextPlugins, pluginName];
  }, plugins);
}
