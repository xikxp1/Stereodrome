const app = require("./app.json");

module.exports = () => {
  const expo = app.expo;
  const extra = expo.extra || {};
  const eas = extra.eas || {};
  const ios = expo.ios || {};
  const slug = process.env.EAS_PROJECT_SLUG || expo.slug;
  const projectId = process.env.EAS_PROJECT_ID || eas.projectId;
  const buildNumber = process.env.IOS_BUILD_NUMBER || ios.buildNumber || "1";

  return {
    ...expo,
    slug,
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
