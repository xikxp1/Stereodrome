const {
  withDangerousMod,
  withGradleProperties,
} = require("expo/config-plugins");
const fs = require("node:fs/promises");
const path = require("node:path");

const androidMinSdkVersion = "26";
const gradleVersion = "9.4.1";

module.exports = function withStereodromeCore(config) {
  const configWithGradleProperties = withGradleProperties(
    config,
    (modConfig) => {
      setGradleProperty(
        modConfig.modResults,
        "android.minSdkVersion",
        androidMinSdkVersion
      );
      return modConfig;
    }
  );

  return withGradleWrapperVersion(configWithGradleProperties);
};

function withGradleWrapperVersion(config) {
  return withDangerousMod(config, [
    "android",
    async (modConfig) => {
      const wrapperPath = path.join(
        modConfig.modRequest.platformProjectRoot,
        "gradle/wrapper/gradle-wrapper.properties"
      );
      const contents = await fs.readFile(wrapperPath, "utf8");
      const distributionPattern = /gradle-\d+(?:\.\d+)+-(bin|all)\.zip/;

      if (!distributionPattern.test(contents)) {
        throw new Error(
          `Unable to find Gradle distribution URL in ${wrapperPath}`
        );
      }

      const updatedContents = contents.replace(
        distributionPattern,
        `gradle-${gradleVersion}-$1.zip`
      );

      if (updatedContents !== contents) {
        await fs.writeFile(wrapperPath, updatedContents);
      }

      return modConfig;
    },
  ]);
}

function setGradleProperty(properties, key, value) {
  const existing = properties.find((property) => property.key === key);
  if (existing) {
    existing.value = value;
  } else {
    properties.push({ type: "property", key, value });
  }
}
