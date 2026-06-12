const app = require("./app.json");

module.exports = () => {
  const expo = app.expo;
  const extra = expo.extra || {};
  const eas = extra.eas || {};
  const ios = expo.ios || {};
  const slug = expo.slug;
  const projectId = process.env.EAS_PROJECT_ID || eas.projectId;
  const buildNumber = process.env.IOS_BUILD_NUMBER || ios.buildNumber || "1";
  const plugins = withPlugin(expo.plugins || [], "expo-background-task");

  return {
    ...expo,
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

function withPlugin(plugins, pluginName) {
  const hasPlugin = plugins.some((plugin) =>
    Array.isArray(plugin) ? plugin[0] === pluginName : plugin === pluginName
  );

  return hasPlugin ? plugins : [...plugins, pluginName];
}
