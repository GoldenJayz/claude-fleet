package dev.claudefleet.app

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.util.Log
import android.view.View
import android.widget.RemoteViews
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.text.DateFormat
import java.util.Date
import kotlin.math.roundToInt

/**
 * Home screen widget: each paired PC's status, CPU and RAM, whether Claude is working there, and
 * the plan usage. It asks the PCs itself (the same LAN/VPN API and tokens as the app) when it is
 * added, every 30 minutes, when the PC list changes in the app, and when its refresh button is
 * tapped. The app hands over its PC list (see [saveMachines]); it is kept in this app's private
 * storage, like the app's own list.
 */
class FleetWidget : AppWidgetProvider() {
  override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
    refresh(context, goAsync())
  }

  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action == ACTION_REFRESH) refresh(context, goAsync()) else super.onReceive(context, intent)
  }

  private enum class State { ONLINE, OFFLINE, REJECTED }

  private class Pc(
    val name: String,
    val state: State,
    val cpu: Double? = null,
    val ram: Double? = null,
    val working: Int = 0,
    val tasks: Int = 0,
    val plan: JSONObject? = null,
  )

  private fun refresh(context: Context, pending: PendingResult) {
    val app = context.applicationContext
    val manager = AppWidgetManager.getInstance(app)
    val ids = manager.getAppWidgetIds(ComponentName(app, FleetWidget::class.java))
    if (ids.isEmpty()) {
      pending.finish()
      return
    }
    // Show "updating" on whatever is on screen now, then fill in the new numbers.
    val busy = RemoteViews(app.packageName, R.layout.fleet_widget)
    busy.setTextViewText(R.id.widget_updated, "updating…")
    manager.partiallyUpdateAppWidget(ids, busy)

    Thread {
      try {
        val machines = loadMachines(app)
        val results = arrayOfNulls<Pc>(machines.size)
        val workers = machines.mapIndexed { i, m -> Thread { results[i] = fetchWithRetry(m) }.apply { start() } }
        val deadline = System.currentTimeMillis() + 8500
        workers.forEach { it.join((deadline - System.currentTimeMillis()).coerceAtLeast(1)) }
        val pcs = machines.mapIndexed { i, m -> results[i] ?: Pc(m.optString("name", "PC"), State.OFFLINE) }
        manager.updateAppWidget(ids, render(app, pcs, machines.isEmpty()))
      } catch (e: Throwable) {
        Log.w(TAG, "refresh failed", e) // leave the last good view up
      } finally {
        pending.finish()
      }
    }.start()
  }

  /** One retry: the first connection after the phone (or its VPN) wakes up often times out. */
  private fun fetchWithRetry(m: JSONObject): Pc {
    val first = fetch(m)
    return if (first.state == State.OFFLINE) fetch(m) else first
  }

  private fun fetch(m: JSONObject): Pc {
    val name = m.optString("name", "PC")
    return try {
      val conn = URL("http://${m.getString("address")}/v1/snapshot").openConnection() as HttpURLConnection
      conn.connectTimeout = 2500
      conn.readTimeout = 3000
      conn.setRequestProperty("Authorization", "Bearer ${m.getString("token")}")
      try {
        when (conn.responseCode) {
          401 -> Pc(name, State.REJECTED)
          200 -> {
            val s = JSONObject(conn.inputStream.bufferedReader().use { it.readText() })
            val metrics = s.optJSONObject("metrics")
            val memTotal = metrics?.optDouble("memTotal", 0.0) ?: 0.0
            val tasks = s.optJSONArray("tasks")
            var activeTasks = 0
            if (tasks != null) for (i in 0 until tasks.length()) {
              val st = tasks.optJSONObject(i)?.optString("status")
              if (st == "running" || st == "queued") activeTasks++
            }
            Pc(
              name = s.optJSONObject("machine")?.optString("machineName")?.takeIf { it.isNotBlank() } ?: name,
              state = State.ONLINE,
              cpu = metrics?.optDouble("cpu"),
              ram = if (memTotal > 0) metrics!!.optDouble("memUsed", 0.0) * 100 / memTotal else null,
              working = s.optJSONObject("claude")?.optJSONArray("working")?.length() ?: 0,
              tasks = activeTasks,
              plan = s.optJSONObject("plan"),
            )
          }
          else -> Pc(name, State.OFFLINE).also { Log.w(TAG, "$name: HTTP ${conn.responseCode}") }
        }
      } finally {
        conn.disconnect()
      }
    } catch (e: Exception) {
      Log.w(TAG, "$name: $e")
      Pc(name, State.OFFLINE)
    }
  }

  private fun render(ctx: Context, pcs: List<Pc>, noMachines: Boolean): RemoteViews {
    val v = RemoteViews(ctx.packageName, R.layout.fleet_widget)
    v.setOnClickPendingIntent(R.id.widget_root, openApp(ctx))
    v.setOnClickPendingIntent(R.id.widget_refresh, refreshIntent(ctx))
    val time = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date())
    val online = pcs.count { it.state == State.ONLINE }
    v.setTextViewText(R.id.widget_updated, if (noMachines) time else "$online of ${pcs.size} online · $time")

    // Plan usage: the freshest reading any PC has (they share one Claude plan).
    val plan = pcs.mapNotNull { it.plan }.maxByOrNull { it.optLong("capturedAtMs") }
    val fiveHour = plan?.optJSONObject("fiveHour")?.optDouble("usedPercentage")
    val weekly = plan?.optJSONObject("sevenDay")?.optDouble("usedPercentage")
    v.setViewVisibility(R.id.widget_plan, if (fiveHour == null && weekly == null) View.GONE else View.VISIBLE)
    planPill(v, R.id.plan_5h, R.id.plan_5h_text, R.id.plan_5h_bar, fiveHour)
    planPill(v, R.id.plan_week, R.id.plan_week_text, R.id.plan_week_bar, weekly)

    v.removeAllViews(R.id.widget_rows)
    if (noMachines) {
      v.setViewVisibility(R.id.widget_empty, View.VISIBLE)
      return v
    }
    v.setViewVisibility(R.id.widget_empty, View.GONE)
    for (pc in pcs.take(8)) {
      val row = RemoteViews(ctx.packageName, R.layout.fleet_widget_row)
      row.setTextViewText(R.id.row_name, pc.name)
      val status = when (pc.state) {
        State.REJECTED -> "token rejected: pair again"
        State.OFFLINE -> "offline"
        State.ONLINE -> when {
          pc.working > 0 && pc.tasks > 0 -> "Claude working · ${plural(pc.tasks, "agent task")}"
          pc.working > 1 -> "Claude working · ${pc.working}"
          pc.working == 1 -> "Claude working"
          pc.tasks > 0 -> plural(pc.tasks, "agent task")
          else -> "idle"
        }
      }
      val dot = when {
        pc.state == State.REJECTED -> R.drawable.widget_dot_amber
        pc.state == State.OFFLINE -> R.drawable.widget_dot_grey
        pc.working > 0 || pc.tasks > 0 -> R.drawable.widget_dot_violet
        else -> R.drawable.widget_dot_green
      }
      row.setImageViewResource(R.id.row_dot, dot)
      row.setTextViewText(R.id.row_status, status)
      if (pc.state == State.ONLINE && pc.cpu != null) {
        row.setViewVisibility(R.id.row_meters, View.VISIBLE)
        meter(row, R.id.row_cpu_bar, R.id.row_cpu_text, pc.cpu)
        meter(row, R.id.row_ram_bar, R.id.row_ram_text, pc.ram)
      } else {
        row.setViewVisibility(R.id.row_meters, View.GONE)
      }
      v.addView(R.id.widget_rows, row)
    }
    return v
  }

  private fun plural(n: Int, word: String) = if (n == 1) "1 $word" else "$n ${word}s"

  private fun meter(row: RemoteViews, bar: Int, text: Int, value: Double?) {
    val pct = (value ?: 0.0).coerceIn(0.0, 100.0).roundToInt()
    row.setProgressBar(bar, 100, pct, false)
    row.setTextViewText(text, if (value == null) "–" else "$pct%")
  }

  private fun planPill(v: RemoteViews, pill: Int, text: Int, bar: Int, used: Double?) {
    if (used == null || used.isNaN()) {
      v.setViewVisibility(pill, View.GONE)
      return
    }
    val pct = used.coerceIn(0.0, 100.0).roundToInt()
    v.setViewVisibility(pill, View.VISIBLE)
    v.setTextViewText(text, "$pct%")
    v.setProgressBar(bar, 100, pct, false)
  }

  companion object {
    const val ACTION_REFRESH = "dev.claudefleet.app.WIDGET_REFRESH"
    private const val PREFS = "fleet_widget"
    private const val TAG = "FleetWidget"

    /** Called by the app with its PC list: [{name, address, token}]. */
    fun saveMachines(context: Context, json: String) {
      val clean = JSONArray()
      val list = JSONArray(json)
      for (i in 0 until list.length()) {
        val m = list.optJSONObject(i) ?: continue
        val address = m.optString("address")
        val token = m.optString("token")
        if (address.isBlank() || token.isBlank()) continue
        clean.put(JSONObject().put("name", m.optString("name", address)).put("address", address).put("token", token))
      }
      val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
      if (prefs.getString("machines", null) == clean.toString()) return
      prefs.edit().putString("machines", clean.toString()).apply()
      context.sendBroadcast(Intent(context, FleetWidget::class.java).setAction(ACTION_REFRESH))
    }

    private fun loadMachines(context: Context): List<JSONObject> = try {
      val a = JSONArray(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("machines", "[]"))
      (0 until a.length()).mapNotNull { a.optJSONObject(it) }
    } catch (_: Exception) {
      emptyList()
    }

    private fun openApp(ctx: Context): PendingIntent = PendingIntent.getActivity(
      ctx, 0,
      Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun refreshIntent(ctx: Context): PendingIntent = PendingIntent.getBroadcast(
      ctx, 1,
      Intent(ctx, FleetWidget::class.java).setAction(ACTION_REFRESH),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
  }
}
