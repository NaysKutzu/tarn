package app.tarn.desktop

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Keeps playback alive in the background and shows the media notification
 * (lock screen / shade controls). State is pulled from Rust once a second.
 */
class PlaybackService : Service() {
  private lateinit var session: MediaSession
  private val handler = Handler(Looper.getMainLooper())

  private var inForeground = false
  private var lastId = ""
  private var lastPlaying: Boolean? = null
  private var lastTitle = ""
  private var cover: Bitmap? = null
  private var coverFor = ""
  private var dirty = false

  private val tick = object : Runnable {
    override fun run() {
      try {
        refresh()
      } catch (_: Throwable) {
      }
      handler.postDelayed(this, 1000)
    }
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    val nm = getSystemService(NotificationManager::class.java)
    nm.createNotificationChannel(
      NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW).apply {
        setShowBadge(false)
      }
    )
    session = MediaSession(this, "Tarn").apply {
      setCallback(object : MediaSession.Callback() {
        override fun onPlay() = act("play")
        override fun onPause() = act("pause")
        override fun onSkipToNext() = act("next")
        override fun onSkipToPrevious() = act("prev")
        override fun onStop() = act("stop")
        override fun onSeekTo(pos: Long) = act("seek:" + (pos / 1000.0))
      })
      isActive = true
    }
    // Must call startForeground soon after startForegroundService().
    goForeground(buildNotification("Tarn", "", false))
    handler.post(tick)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    intent?.getStringExtra(EXTRA_ACTION)?.let { act(it) }
    return START_STICKY
  }

  override fun onDestroy() {
    handler.removeCallbacks(tick)
    session.release()
    super.onDestroy()
  }

  private fun act(a: String) {
    try {
      MediaBridge.nativeAction(a)
    } catch (_: Throwable) {
    }
  }

  private fun refresh() {
    val st = JSONObject(MediaBridge.nativeState())
    if (st.optBoolean("login", false)) {
      startActivity(Intent(this, LoginActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
    if (!st.optBoolean("has", false)) {
      if (inForeground && lastId.isNotEmpty()) {
        stopForeground(STOP_FOREGROUND_REMOVE)
        inForeground = false
      }
      lastId = ""
      lastPlaying = null
      return
    }
    val id = st.optString("id")
    val title = st.optString("title")
    val artist = st.optString("artist")
    val playing = st.optBoolean("playing", false)
    val position = (st.optDouble("position", 0.0) * 1000).toLong()
    val duration = (st.optDouble("duration", 0.0) * 1000).toLong()
    val coverUrl = if (st.isNull("cover")) "" else st.optString("cover")

    val changed = id != lastId || playing != lastPlaying || title != lastTitle || dirty
    dirty = false
    if (id != lastId) {
      cover = null
      coverFor = ""
      fetchCover(id, coverUrl)
    }

    if (changed || duration > 0) {
      val md = MediaMetadata.Builder()
        .putString(MediaMetadata.METADATA_KEY_TITLE, title)
        .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
        .putString(MediaMetadata.METADATA_KEY_ALBUM, if (st.isNull("album")) "" else st.optString("album"))
      if (duration > 0) md.putLong(MediaMetadata.METADATA_KEY_DURATION, duration)
      cover?.let { md.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, it) }
      session.setMetadata(md.build())
    }
    session.setPlaybackState(
      PlaybackState.Builder()
        .setActions(
          PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS or
            PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_STOP
        )
        .setState(
          if (playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
          position,
          if (playing) 1f else 0f
        )
        .build()
    )

    if (changed || !inForeground) {
      goForeground(buildNotification(title, artist, playing))
    }
    lastId = id
    lastPlaying = playing
    lastTitle = title
  }

  private fun fetchCover(id: String, url: String) {
    if (!url.startsWith("http")) return
    Thread {
      try {
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = 8000
        c.readTimeout = 8000
        val bmp = c.inputStream.use { BitmapFactory.decodeStream(it) }
        handler.post {
          if (bmp != null && id == lastId) {
            cover = bmp
            coverFor = id
            dirty = true // re-send metadata + notification with the art
          }
        }
      } catch (_: Throwable) {
      }
    }.start()
  }

  private fun goForeground(n: Notification) {
    if (Build.VERSION.SDK_INT >= 29) {
      startForeground(NOTIF_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
    } else {
      startForeground(NOTIF_ID, n)
    }
    inForeground = true
  }

  private fun action(icon: Int, label: String, a: String, code: Int): Notification.Action {
    val pi = PendingIntent.getService(
      this, code,
      Intent(this, PlaybackService::class.java).putExtra(EXTRA_ACTION, a),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    return Notification.Action.Builder(Icon.createWithResource(this, icon), label, pi).build()
  }

  private fun buildNotification(title: String, artist: String, playing: Boolean): Notification {
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    val b = Notification.Builder(this, CHANNEL)
      .setSmallIcon(android.R.drawable.ic_media_play)
      .setContentTitle(title)
      .setContentText(artist)
      .setContentIntent(open)
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      .setOnlyAlertOnce(true)
      .setOngoing(playing)
      .addAction(action(android.R.drawable.ic_media_previous, "Previous", "prev", 1))
      .addAction(
        if (playing) action(android.R.drawable.ic_media_pause, "Pause", "pause", 2)
        else action(android.R.drawable.ic_media_play, "Play", "play", 2)
      )
      .addAction(action(android.R.drawable.ic_media_next, "Next", "next", 3))
      .setStyle(
        Notification.MediaStyle()
          .setMediaSession(session.sessionToken)
          .setShowActionsInCompactView(0, 1, 2)
      )
    cover?.let { b.setLargeIcon(it) }
    return b.build()
  }

  companion object {
    const val CHANNEL = "playback"
    const val NOTIF_ID = 4242
    const val EXTRA_ACTION = "tarn.action"
  }
}
