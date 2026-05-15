const { withGradleProperties } = require("expo/config-plugins");

const androidMinSdkVersion = "26";

module.exports = function withStereodromeCore(config) {
  return withGradleProperties(config, (config) => {
    setGradleProperty(
      config.modResults,
      "android.minSdkVersion",
      androidMinSdkVersion
    );
    return config;
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
