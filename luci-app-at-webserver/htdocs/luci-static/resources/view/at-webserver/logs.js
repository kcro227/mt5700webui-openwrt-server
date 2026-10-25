'use strict';
'require view';
'require fs';
'require ui';
'require uci';

var MAX_LINES = 300;
var REFRESH_MS = 10000;

function parseLines(content) {
	var lines = (content || '').trim().split('\n');
	if (lines.length > MAX_LINES)
		lines = lines.slice(lines.length - MAX_LINES);
	return lines;
}

function kindOf(line) {
	var m = line.match(/\[[^\]]+\]\s*\[([^\]]+)\]/);
	return m ? m[1] : '';
}

function colorOf(kind) {
	switch (kind) {
	case '网络': return '#4fc3f7';
	case '短信': return '#81c784';
	case '来电': return '#ffb74d';
	default: return '#e6e6e6';
	}
}

return view.extend({
	load: function() {
		return uci.load('at-webserver').then(function() {
			var logFile = uci.get('at-webserver', 'config', 'log_file') || '';
			if (!logFile)
				return { path: '', content: '', status: 'unconfigured' };

			return fs.read(logFile).then(function(content) {
				return { path: logFile, content: content || '', status: 'ok' };
			}).catch(function(err) {
				return { path: logFile, content: '', status: 'error', error: err && err.message ? err.message : 'failed' };
			});
		});
	},

	handleClearLog: function(ev) {
		return L.Request.get('/cgi-bin/at-log-clear').then(function(res) {
			try {
				var result = res.json();
				if (result.success) {
					ui.addNotification(null, E('p', {}, _('✓ 通知日志已清空')), 'success');
					setTimeout(function() { window.location.reload(); }, 600);
				} else {
					ui.addNotification(null, E('p', {}, _('清空失败: %s').format(result.error || '未知错误')), 'error');
				}
			} catch(e) {
				ui.addNotification(null, E('p', {}, _('清空失败: 解析响应出错')), 'error');
			}
		}).catch(function(err) {
			ui.addNotification(null, E('p', {}, _('清空失败: %s').format(err.message || '请求失败')), 'error');
		});
	},

	render: function(data) {
		var notif = data || { path: '', content: '', status: 'unconfigured' };
		var lines = parseLines(notif.content);
		var filter = '';
		var autoRefresh = true;
		var timer = null;
		var pre = null;
		var countEl = null;

		function paint() {
			var shown = lines.filter(function(line) {
				return !filter || kindOf(line) === filter;
			});

			while (pre.firstChild)
				pre.removeChild(pre.firstChild);

			if (!shown.length) {
				pre.appendChild(document.createTextNode(_('暂无匹配的记录')));
			} else {
				shown.slice().reverse().forEach(function(line, index) {
					if (index)
						pre.appendChild(document.createTextNode('\n'));
					pre.appendChild(E('span', { style: 'color:' + colorOf(kindOf(line)) }, line));
				});
			}

			if (countEl)
				countEl.textContent = _('共 %d 条记录（显示 %d 条，最新 300 条）').format(lines.length, shown.length);
		}

		function reload(showNotice) {
			if (!notif.path)
				return;
			return fs.read(notif.path).then(function(content) {
				lines = parseLines(content);
				paint();
				if (showNotice)
					ui.addNotification(null, E('p', {}, _('日志已刷新')), 'info');
			}).catch(function(err) {
				if (showNotice)
					ui.addNotification(null, E('p', {}, _('读取日志失败: %s').format(err && err.message ? err.message : 'failed')), 'error');
			});
		}

		var notifStyle = 'background:#1e1e1e; color:#e6e6e6; border:1px solid #333; padding:15px; border-radius:4px; max-height:600px; overflow-y:auto; font-family:monospace; font-size:13px; line-height:1.6; white-space: pre-wrap; word-wrap: break-word;';

		var view = E('div', { 'class': 'cbi-map' }, [
			E('h2', {}, _('通知日志')),
			E('div', { 'class': 'cbi-section' }, [
				E('div', { 'class': 'cbi-section-descr' },
					_('查看短信、来电、网络变化等通知记录（颜色：蓝=网络，绿=短信，橙=来电）'))
			])
		]);

		if (notif.status === 'unconfigured') {
			view.appendChild(E('div', { 'class': 'cbi-section' }, [
				E('div', { 'class': 'alert-message warning' }, [
					E('h4', {}, _('未配置日志文件')),
					E('p', {}, _('请先在配置页面中设置日志文件路径，例如：/var/log/at-notifications.log')),
					E('p', {}, [
						E('a', {
							'href': L.url('admin/services/at-webserver/config'),
							'class': 'btn cbi-button-apply'
						}, _('前往配置'))
					])
				])
			]));
			return view;
		}

		if (notif.status === 'error') {
			view.appendChild(E('div', { 'class': 'cbi-section' }, [
				E('div', { 'class': 'alert-message error' }, [
					E('h4', {}, _('读取日志失败')),
					E('p', {}, _('错误信息: %s').format(notif.error || '未知错误')),
					E('p', {}, _('日志路径: %s').format(notif.path)),
					E('p', {}, _('请检查文件权限或路径是否正确'))
				])
			]));
			return view;
		}

		countEl = E('span', { style: 'color:#666; font-size:13px; margin-left:8px' }, '');

		var toolbar = E('div', { style: 'display:flex; flex-wrap:wrap; gap:10px; align-items:center; margin-bottom:10px' }, [
			E('label', { style: 'display:flex; align-items:center; gap:6px' }, [
				_('筛选'),
				E('select', {
					'class': 'cbi-input-select',
					'change': function(ev) { filter = ev.target.value; paint(); }
				}, [
					E('option', { value: '' }, _('全部')),
					E('option', { value: '网络' }, _('网络')),
					E('option', { value: '短信' }, _('短信')),
					E('option', { value: '来电' }, _('来电'))
				])
			]),
			E('label', { style: 'display:flex; align-items:center; gap:6px' }, [
				E('input', {
					'type': 'checkbox',
					'checked': true,
					'change': function(ev) { autoRefresh = ev.target.checked; }
				}),
				_('自动刷新')
			]),
			E('button', {
				'class': 'btn cbi-button-apply',
				'click': function() { reload(true); }
			}, _('立即刷新')),
			E('button', {
				'class': 'btn cbi-button-reset',
				'click': ui.createHandlerFn(this, 'handleClearLog')
			}, _('清空日志')),
			countEl
		]);

		pre = E('pre', { style: notifStyle }, '');

		view.appendChild(E('div', { 'class': 'cbi-section' }, [
			E('div', { 'class': 'cbi-value' }, [
				E('label', { 'class': 'cbi-value-title' }, _('日志路径')),
				E('div', { 'class': 'cbi-value-field' }, [ E('code', {}, notif.path) ])
			]),
			toolbar
		]));

		view.appendChild(E('div', { 'class': 'cbi-section' }, [ pre ]));

		paint();

		timer = window.setInterval(function() {
			if (!document.body.contains(pre)) {
				window.clearInterval(timer);
				return;
			}
			if (autoRefresh)
				reload(false);
		}, REFRESH_MS);

		return view;
	},

	handleSaveApply: null,
	handleSave: null,
	handleReset: null
});
