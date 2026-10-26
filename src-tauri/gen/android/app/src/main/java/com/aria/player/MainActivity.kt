package com.aria.player

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var activeWebView: WebView? = null
  private var audioDeviceCallback: AudioDeviceCallback? = null

  companion object {
    @JvmStatic
    var instance: MainActivity? = null

    @JvmStatic
    fun openBrowser(url: String): Boolean {
      val act = instance ?: return false
      return try {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        act.startActivity(intent)
        true
      } catch (e: Exception) {
        e.printStackTrace()
        false
      }
    }

    @JvmStatic
    fun copyToClipboard(text: String): Boolean {
      val act = instance ?: return false
      return try {
        act.runOnUiThread {
          val clipboard = act.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
          val clip = ClipData.newPlainText("Aria", text)
          clipboard?.setPrimaryClip(clip)
        }
        true
      } catch (e: Exception) {
        e.printStackTrace()
        false
      }
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    instance = this
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    AriaBluetoothAudioHelper.initialize(this)
    registerAudioDeviceMonitor()
  }

  private fun registerAudioDeviceMonitor() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
      val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
      val callback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>?) {
          notifyAudioDevicesChanged()
        }

        override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>?) {
          notifyAudioDevicesChanged()
        }
      }
      audioManager.registerAudioDeviceCallback(callback, null)
      audioDeviceCallback = callback
    }
  }

  private fun notifyAudioDevicesChanged() {
    runOnUiThread {
      activeWebView?.evaluateJavascript(
        "window.dispatchEvent(new CustomEvent('aria-audio-devices-changed'));",
        null
      )
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    activeWebView = webView
    webView.addJavascriptInterface(object {
      @JavascriptInterface
      fun openUrl(url: String): Boolean {
        return openBrowser(url)
      }

      @JavascriptInterface
      fun copyText(text: String): Boolean {
        return copyToClipboard(text)
      }

      @JavascriptInterface
      fun hasUsbAudioDevice(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return false
        val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return false
        val devices = audioManager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        return devices.any {
          it.type == AudioDeviceInfo.TYPE_USB_DEVICE || it.type == AudioDeviceInfo.TYPE_USB_HEADSET
        }
      }

      @JavascriptInterface
      fun hasBluetoothAudioDevice(): Boolean {
        return AriaBluetoothAudioHelper.hasBluetoothOutput(this@MainActivity)
      }

      @JavascriptInterface
      fun getBluetoothAudioInfo(): String {
        return AriaBluetoothAudioHelper.getActiveBluetoothDescription(this@MainActivity) ?: ""
      }

      @JavascriptInterface
      fun optimizeBluetoothAudio(): String {
        return AriaBluetoothAudioHelper.optimizeBluetoothAudio(this@MainActivity) ?: ""
      }
    }, "AriaAndroid")
  }

  override fun onDestroy() {
    AriaBluetoothAudioHelper.cleanup()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && audioDeviceCallback != null) {
      val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager
      audioManager?.unregisterAudioDeviceCallback(audioDeviceCallback)
      audioDeviceCallback = null
    }
    activeWebView = null
    if (instance === this) {
      instance = null
    }
    super.onDestroy()
  }
}
