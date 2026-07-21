package expo.modules.stereodromecore

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NowPlayingInfoTest {
  @Test
  fun playingProjectionToleratesMonitorPositionDrift() {
    val current = projection(positionSeconds = 10.0, isPlaying = true)
    val next = projection(positionSeconds = 10.1, isPlaying = true)

    assertTrue(current.hasSameProjection(next))
  }

  @Test
  fun pausedProjectionKeepsExactSeekPosition() {
    val current = projection(positionSeconds = 10.0, isPlaying = false)
    val next = projection(positionSeconds = 10.1, isPlaying = false)

    assertFalse(current.hasSameProjection(next))
  }

  @Test
  fun projectionIncludesCapabilitiesAndMetadata() {
    val current = projection()

    assertFalse(current.hasSameProjection(current.copy(canNext = false)))
    assertFalse(current.hasSameProjection(current.copy(title = "Changed")))
    assertFalse(current.hasSameProjection(current.copy(queueIndex = 2)))
  }

  private fun projection(
    positionSeconds: Double = 10.0,
    isPlaying: Boolean = true,
  ) = NowPlayingInfo(
    songId = "song",
    title = "Title",
    artist = "Artist",
    album = "Album",
    durationSeconds = 180.0,
    positionSeconds = positionSeconds,
    isPlaying = isPlaying,
    artworkUri = "file:///artwork.jpg",
    queueIndex = 1,
    queueCount = 3,
    canNext = true,
    canPlay = true,
    canPrevious = true,
    canSeek = true,
  )
}
