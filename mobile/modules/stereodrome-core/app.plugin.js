const { withGradleProperties } = require("expo/config-plugins");

const androidMinSdkVersion = "26";

module.exports = function withStereodromeCore(config) {
  return withGradleProperties(config, (modConfig) => {
    setGradleProperty(
      modConfig.modResults,
      "android.minSdkVersion",
      androidMinSdkVersion
    );
    return modConfig;
  });
};

function setGradleProperty(properties, key, value) {
  const existing = properties.find((property) => property.key === key);
  if (existing) {
    existing.value = value;
  } else {
    properties.push({ type: "property", key, value });
  }
}
